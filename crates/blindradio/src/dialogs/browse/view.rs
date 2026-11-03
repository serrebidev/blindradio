//! The Browse window itself; `super` holds its model.

use super::*;

/// The wx tree over the model. Every change the program makes runs `quiet`; handlers return
/// at once then, before borrowing the app or the model.
struct View {
	tree: TreeCtrl,
	root: TreeItemId,
	items: RefCell<HashMap<usize, TreeItemId>>,
	model: RefCell<BrowseModel>,
	quiet: Cell<bool>,
	sort: Cell<Sort>,
}

impl View {
	fn index(&self, item: &TreeItemId) -> Option<usize> {
		self.tree
			.get_custom_data_direct(item)?
			.downcast_ref::<usize>()
			.copied()
	}

	fn item(&self, i: usize) -> Option<TreeItemId> {
		self.items.borrow().get(&i).cloned()
	}

	fn quietly<R>(&self, f: impl FnOnce() -> R) -> R {
		let was = self.quiet.replace(true);
		let r = f();
		self.quiet.set(was);
		r
	}

	/// The wx children of `parent`, in the tree's own order.
	fn wx_children(&self, parent: &TreeItemId) -> Vec<TreeItemId> {
		let mut out = Vec::new();
		if let Some((first, mut cookie)) = self.tree.get_first_child(parent) {
			out.push(first);
			while let Some(next) = self.tree.get_next_child(parent, &mut cookie) {
				out.push(next);
			}
		}
		out
	}

	/// Makes the wx children of `parent` match the model.
	fn sync(&self, parent: Option<usize>) {
		self.quietly(|| {
			let m = self.model.borrow();
			let wx_parent = match parent {
				Some(p) => match self.item(p) {
					Some(item) => item,
					None => return,
				},
				None => self.root.clone(),
			};
			let want = m.children(parent);
			let keep: HashSet<usize> = want.iter().copied().collect();
			for item in self.wx_children(&wx_parent) {
				let i = self.index(&item);
				if i.is_none_or(|i| !keep.contains(&i)) {
					if let Some(i) = i {
						self.forget(&m, i);
					}
					self.tree.delete(&item);
				}
			}
			// A reorder leaves the same rows in a new order, and the
			// tree cannot move a row: updating labels in place showed
			// the old order until the dialog reopened (the folder
			// move only looked right because a folder's key carries
			// its index, so the names swapped instead). When the
			// tree's order differs from the model's, rebuild the
			// level in the model's order; folders the model holds
			// open are expanded again below.
			let have: Vec<usize> = self
				.wx_children(&wx_parent)
				.iter()
				.filter_map(|item| self.index(item))
				.collect();
			if have.as_slice() != want {
				for item in self.wx_children(&wx_parent) {
					if let Some(i) = self.index(&item) {
						self.forget(&m, i);
					}
					self.tree.delete(&item);
				}
			}
			let mut prev: Option<TreeItemId> = None;
			for &c in want {
				let label = m.display_label(c);
				let item = match self.item(c) {
					Some(item) => {
						if self.tree.get_item_text(&item).as_deref() != Some(label.as_str()) {
							self.tree.set_item_text(&item, &label);
						}
						item
					}
					None => {
						let item = match &prev {
							Some(p) => self.tree.insert_item(&wx_parent, p, &label, None, None),
							None => self.tree.prepend_item(&wx_parent, &label, None, None),
						};
						let Some(item) = item else { continue };
						self.tree.set_custom_data_direct(&item, c);
						self.items.borrow_mut().insert(c, item.clone());
						item
					}
				};
				self.tree.set_item_has_children(&item, m.has_children(c));
				prev = Some(item);
			}
			// Rows rebuilt by the reorder pass come back collapsed;
			// open the ones the model holds open again.
			for &c in want {
				if m.expanded(c)
					&& let Some(item) = self.item(c)
				{
					self.tree.expand(&item);
				}
			}
		});
	}

	/// Drops `i` and everything under it from the map: wx deletes them together.
	fn forget(&self, m: &BrowseModel, i: usize) {
		self.items.borrow_mut().remove(&i);
		for &c in m.children(Some(i)) {
			self.forget(m, c);
		}
	}

	/// `sync` all the way down.
	fn sync_deep(&self, parent: Option<usize>) {
		self.sync(parent);
		let kids = self.model.borrow().children(parent).to_vec();
		for c in kids {
			if self.model.borrow().is_folder(c) {
				self.sync_deep(Some(c));
			}
		}
	}

	/// Opens in wx what is open in the model.
	fn expand_open(&self, parent: Option<usize>) {
		let kids = self.model.borrow().children(parent).to_vec();
		for c in kids {
			if self.model.borrow().expanded(c) {
				if let Some(item) = self.item(c) {
					self.quietly(|| self.tree.expand(&item));
				}
				self.expand_open(Some(c));
			}
		}
	}

	/// Selects the model's focus if wx is elsewhere; only after the program changed the tree.
	fn show_focus(&self) {
		let Some(f) = self.model.borrow().focus() else {
			return;
		};
		let on = self.tree.get_selection().and_then(|s| self.index(&s));
		if on != Some(f)
			&& let Some(item) = self.item(f)
		{
			self.quietly(|| {
				self.tree.select_item(&item);
				self.tree.ensure_visible(&item);
			});
		}
	}

	/// Focus arrived at `i` by the user: a station tunes.
	fn focused(&self, app: &Rc<RefCell<App>>, i: usize) {
		let dial = {
			let mut m = self.model.borrow_mut();
			m.set_focus(i);
			m.dial(i)
		};
		// Paste follows the row the user is standing on, folder and
		// playlist rows included, not only stations.
		let home = self.model.borrow().paste_home(i);
		let mut a = app.borrow_mut();
		a.paste_home = home.map(|(id, path)| crate::app::PasteDest::Collection(id, path));
		match dial {
			Some((stations, k)) => a.focus_station(&stations, k),
			None => a.selected = None,
		}
	}

	/// A user move made by the program (Home, End, a letter).
	fn go(&self, app: &Rc<RefCell<App>>, i: usize) {
		self.model.borrow_mut().set_focus(i);
		self.show_focus();
		self.focused(app, i);
	}

	fn open(self: &Rc<Self>, app: &Rc<RefCell<App>>, token: &WindowToken, i: usize) {
		let fetch = self.model.borrow_mut().open(i);
		self.sync(Some(i));
		self.sync(self.model.borrow().parent(i));
		if let Some(id) = fetch {
			self.fetch(&app.borrow(), token, i, id);
		}
	}

	fn fetch(self: &Rc<Self>, app: &App, token: &WindowToken, i: usize, id: String) {
		let sources = app.sources.clone();
		let sort = self.sort.get();
		let v = self.clone();
		app.bridge.spawn(
			token.clone(),
			async move {
				let r = sources.children(&id, sort).await;
				(id, r)
			},
			move |app: &mut App, (id, r)| {
				// The sort changed while this was in flight: its order is stale.
				if v.sort.get() != sort {
					return;
				}
				let nodes = r.inspect_err(|e| eprintln!("browse {id}: {e}")).ok();
				let (more, parent) = {
					let m = v.model.borrow();
					(m.is_load_more(i), m.parent(i))
				};
				let said = v.model.borrow_mut().loaded(i, nodes);
				if more {
					v.sync(parent);
				} else {
					v.sync(Some(i));
					v.sync(parent);
					if !v.model.borrow().expanded(i)
						&& let Some(item) = v.item(i)
					{
						v.quietly(|| v.tree.collapse(&item));
					}
				}
				v.show_focus();
				if let Some(text) = said {
					app.say(&text);
				}
			},
		);
	}

	/// A new sort from the controls: Radio Browser levels load again in the new order.
	fn resort(self: &Rc<Self>, sort: Sort) {
		if self.sort.get() == sort {
			return;
		}
		self.sort.set(sort);
		self.model.borrow_mut().resort();
		self.sync_deep(None);
		if let Some(item) = self.item(self.model.borrow().top()) {
			self.quietly(|| self.tree.collapse(&item));
		}
		self.show_focus();
	}

	/// Enter or a double click. Space is Play/Pause, taken before the tree sees it.
	fn activate(
		self: &Rc<Self>,
		app: &Rc<RefCell<App>>,
		token: &WindowToken,
		dialog: Dialog,
		i: usize,
	) {
		let (station, folder, expanded) = {
			let m = self.model.borrow();
			(m.station(i).cloned(), m.is_folder(i), m.expanded(i))
		};
		if let Some(st) = station {
			app.borrow_mut().selected = Some(st);
			dispatch(app, Action::Play);
			if app.borrow().settings.close_lists_when_playing {
				dialog.end_modal(ID_OK);
			}
		} else if folder {
			let Some(item) = self.item(i) else { return };
			if expanded {
				self.model.borrow_mut().close(i);
				self.quietly(|| self.tree.collapse(&item));
			} else {
				self.open(app, token, i);
				self.quietly(|| self.tree.expand(&item));
			}
		} else {
			let fetch = self.model.borrow_mut().more(i);
			if let Some(id) = fetch {
				self.sync(self.model.borrow().parent(i));
				self.fetch(&app.borrow(), token, i, id);
			}
		}
	}

	/// Shift+Up / Shift+Down (Alt works too): move the focused folder
	/// one step among the folders on its level. Local folders and
	/// collections reorder the library itself; remote folders and the
	/// roots record the order in the settings, since the source's order
	/// is not ours to change.
	fn move_folder(&self, app: &Rc<RefCell<App>>, up: bool) {
		let (f, parent, local) = {
			let m = self.model.borrow();
			let Some(f) = m.focus() else { return };
			if !m.is_folder(f) {
				return;
			}
			(f, m.parent(f), m.is_local(f) && m.parent(f).is_some())
		};
		if local {
			// A folder's collection and path, or a collection's own row.
			let Some((collection, path)) = self.model.borrow().paste_home(f) else {
				return;
			};
			let old_pos = self
				.model
				.borrow()
				.children(parent)
				.iter()
				.position(|&x| x == f);
			let moved = {
				let mut a = app.borrow_mut();
				let r = a.library.move_at(&collection, &path, up);
				if r.is_some() {
					a.save_library();
				}
				r
			};
			if moved.is_none() {
				return;
			}
			self.refresh_local(app);
			// Local levels list stations first, then folders, so the
			// moved row sits exactly one level slot over.
			if let Some(old) = old_pos {
				let level = self.model.borrow().children(parent).to_vec();
				let at = if up { old.wrapping_sub(1) } else { old + 1 };
				if let Some(&to) = level.get(at) {
					self.model.borrow_mut().set_focus(to);
					self.show_focus();
					self.focused(app, to);
				}
			}
		} else if let Some((key, order)) = self.model.borrow_mut().move_folder(f, up) {
			{
				let mut a = app.borrow_mut();
				a.settings.browse_order.insert(key, order);
				a.save_settings();
			}
			// The rebuilt level lost what was under its open rows.
			self.sync_deep(parent);
			self.expand_open(parent);
			self.show_focus();
		}
	}

	/// Shift+Up / Shift+Down (Alt works too) on a station: move it
	/// one step among the stations on its level, the same gesture
	/// folders already had. Only local stations move — a remote
	/// directory's order is the source's, not ours to change. The
	/// library itself is reordered, so the main window's list takes
	/// the new order as soon as Browse closes.
	fn move_station(&self, app: &Rc<RefCell<App>>, up: bool) {
		let spot = {
			let m = self.model.borrow();
			let Some(f) = m.focus() else { return };
			m.local_station(f)
		};
		let Some((collection, path, index)) = spot else {
			return;
		};
		let landed = {
			let mut a = app.borrow_mut();
			let r = a
				.library
				.collection_mut(&collection)
				.and_then(|c| c.move_station(&path, index, up));
			if r.is_some() {
				a.save_library();
			}
			r
		};
		let Some(landed) = landed else { return };
		self.refresh_local(app);
		// Focus follows the station to its new slot on the level.
		let to = {
			let m = self.model.borrow();
			m.local_row(&collection, &path).and_then(|parent| {
				m.children(Some(parent))
					.iter()
					.filter(|&&x| m.station(x).is_some())
					.nth(landed)
					.copied()
			})
		};
		if let Some(to) = to {
			self.model.borrow_mut().set_focus(to);
			self.show_focus();
			self.focused(app, to);
		}
	}

	fn delete(&self, app: &Rc<RefCell<App>>, i: usize) {
		let target = self.model.borrow().delete_target(i);
		match target {
			Delete::NotOwn => app.borrow_mut().say(s::ONLY_OWN),
			Delete::Remove {
				collection,
				station,
			} => {
				{
					let mut a = app.borrow_mut();
					a.library.remove_from_collection(&collection, &station);
					a.save_library();
					a.refresh_dial();
				}
				self.refresh_local(app);
				self.tune_focus(app);
			}
			Delete::RemoveFolder { collection, path } => {
				// The Collections window's rule: deleting a folder is
				// confirmed, with what goes with it named.
				let question = {
					let m = self.model.borrow();
					context_menu::delete_question(m.label(i), m.stations_under(i).len())
				};
				if !confirm(&self.tree, &question, &plain(s::DELETE_ITEM)) {
					return;
				}
				{
					let mut a = app.borrow_mut();
					if a.library.remove_at(&collection, &path) {
						a.save_library();
						a.refresh_dial();
					}
				}
				self.refresh_local(app);
			}
		}
	}

	/// After the library changed; focus that went with its item lands nearby without tuning.
	fn refresh_local(&self, app: &Rc<RefCell<App>>) {
		self.model.borrow_mut().refresh_local(&app.borrow().library);
		self.sync(None);
		for root in self.model.borrow().local_roots() {
			self.sync_deep(Some(root));
		}
		// A reorder rebuilt its level closed: the rows under it came back after.
		self.expand_open(None);
		self.show_focus();
		let m = self.model.borrow();
		let mut a = app.borrow_mut();
		a.selected = m.focus().and_then(|f| m.station(f)).cloned();
		a.paste_home = m
			.focus()
			.and_then(|f| m.paste_home(f))
			.map(|(id, path)| crate::app::PasteDest::Collection(id, path));
	}

	fn context_menu(self: &Rc<Self>, app: &Rc<RefCell<App>>, token: &WindowToken) {
		let Some(f) = self.model.borrow().focus() else {
			return;
		};
		let (mut items, station, collection, local_folder, new_folder_in, has_clip, paste_dest) = {
			let m = self.model.borrow();
			let a = app.borrow();
			// Where a New Folder would land: inside a local folder,
			// or at the root of the collection whose row this is (the
			// Custom Stations root is not under Collections, so its
			// own key names it). Remote rows get neither.
			let new_folder_in = m.local_folder(f).or_else(|| {
				let key = m.key(f)?;
				a.library
					.collections
					.iter()
					.any(|c| c.id == key)
					.then(|| (key.to_owned(), Vec::new()))
			});
			let has_clip = a.station_clip.is_some();
			// Where Paste would land for this row: the model's word.
			let paste_dest = m.paste_home(f);
			(
				context_menu::items_for(&m.target(f), &a.library, has_clip),
				m.station(f).cloned(),
				m.collection(f),
				m.local_folder(f),
				new_folder_in,
				has_clip,
				paste_dest,
			)
		};
		// A station in a collection leaves it as the Delete key does. Custom Stations has its own.
		if let Ok(items) = &mut items
			&& let Delete::Remove { collection, .. } = self.model.borrow().delete_target(f)
			&& collection != CUSTOM
		{
			items.push(context_menu::Item::Delete);
		}
		let items = match items {
			Ok(items) => items,
			Err(text) => {
				// A folder inside a playlist is the user's own: it
				// renames, and everything under it exports. Any other
				// header — a category, network or section — only
				// exports, and only when it holds something.
				if local_folder.is_some() {
					let mut items = vec![context_menu::Item::NewFolder];
					if has_clip {
						items.push(context_menu::Item::PasteStation);
					}
					items.extend([
						context_menu::Item::Rename,
						context_menu::Item::Delete,
						context_menu::Item::Export,
					]);
					items
				} else {
					let m = self.model.borrow();
					if m.stations_under(f).is_empty() {
						return app.borrow_mut().say(text);
					}
					vec![context_menu::Item::Export]
				}
			}
		};
		let Some(choice) = context_menu::popup(&self.tree, &items) else {
			return;
		};
		let deleted = station.is_some() && choice == Choice::DeleteCustom;
		match (station, collection, choice) {
			(Some(_), _, Choice::Delete) => self.delete(app, f),
			(Some(st), _, choice) => context_menu::apply(app, &self.tree, &st, choice),
			(None, Some((id, name)), Choice::Export) => {
				let (stations, folders) = {
					let a = app.borrow();
					match a.library.collections.iter().find(|c| c.id == id) {
						Some(c) => (c.stations.clone(), c.folders.clone()),
						None => (Vec::new(), Vec::new()),
					}
				};
				context_menu::export_playlist(app, &self.tree, &name, &stations, &folders);
			}
			(None, Some((id, _)), Choice::NewFolder) => {
				if let Some(name) =
					prompt_text(&self.tree, s::FOLDER_NAME, s::NEW_FOLDER_TITLE)
				{
					let mut a = app.borrow_mut();
					if a.library
						.collection_mut(&id)
						.is_some_and(|c| c.add_folder(&[], &name).is_some())
					{
						a.save_library();
					}
				}
			}
			(None, None, Choice::NewFolder) => {
				if let Some((id, parent)) = &new_folder_in
					&& let Some(name) =
						prompt_text(&self.tree, s::FOLDER_NAME, s::NEW_FOLDER_TITLE)
				{
					let mut a = app.borrow_mut();
					if a.library
						.collection_mut(id)
						.is_some_and(|c| c.add_folder(parent, &name).is_some())
					{
						a.save_library();
					}
				}
			}
			(None, None, Choice::Rename) => {
				if let Some((id, path)) = &local_folder
					&& let Some(name) =
						prompt_text(&self.tree, s::FOLDER_NAME, &plain(s::RENAME_ITEM))
				{
					let mut a = app.borrow_mut();
					let renamed = a
						.library
						.collection_mut(id)
						.is_some_and(|c| c.rename_folder(path, &name));
					if renamed {
						a.save_library();
					}
				}
			}
			(None, Some((id, _)), Choice::PasteStation) => {
				// At the collection's own level, beside its folders:
				// a station need not live in a subfolder.
				app.borrow_mut().paste_into(crate::app::PasteDest::Collection(
					id,
					Vec::new(),
				));
				self.refresh_local(app);
			}
			(None, None, Choice::PasteStation) => {
				if let Some((id, path)) = &paste_dest {
					app.borrow_mut().paste_into(crate::app::PasteDest::Collection(
						id.clone(),
						path.clone(),
					));
					self.refresh_local(app);
				}
			}
			(None, None, Choice::Export) => {
				// A local folder exports from the library itself:
				// complete, however little of it the view has loaded,
				// and grouped by its real subfolders.
				if let Some((id, path)) = &local_folder {
					let data = {
						let a = app.borrow();
						a.library
							.collections
							.iter()
							.find(|c| c.id == *id)
							.and_then(|c| c.folder_at(path))
							.map(|f| (f.stations.clone(), f.folders.clone()))
					};
					if let Some((stations, folders)) = data {
						let name = self.model.borrow().label(f).to_owned();
						return context_menu::export_playlist(
							app,
							&self.tree,
							&name,
							&stations,
							&folders,
						);
					}
				}
				// A remote folder: gather its whole subtree from the
				// source first, then the save dialog gets it with the
				// structure intact — never flattened into one group
				// named after the folder (the Germany export bug).
				let (name, key) = {
					let m = self.model.borrow();
					(m.label(f).to_owned(), m.key(f).map(str::to_owned))
				};
				let Some(key) = key else { return };
				let sources = app.borrow().sources.clone();
				let sort = self.sort.get();
				let (v, a) = (self.clone(), app.clone());
				app.borrow().bridge.spawn(
					token.clone(),
					async move { sources.tree(&key, sort).await },
					move |_: &mut App, r| match r {
						Ok((stations, folders)) => {
							context_menu::export_playlist(&a, &v.tree, &name, &stations, &folders)
						}
						Err(e) => {
							eprintln!("browse export {name}: {e}");
							a.borrow_mut().say(s::NOT_SAVED);
						}
					},
				);
			}
			(None, Some((id, _)), Choice::Rename) => {
				if let Some(name) =
					prompt_text(&self.tree, s::COLLECTION_NAME, &plain(s::RENAME_ITEM))
				{
					let mut a = app.borrow_mut();
					if a.library.rename_collection(&id, &name).is_ok() {
						a.save_library();
					}
				}
			}
			(None, _, Choice::Delete) => self.delete(app, f),
			_ => {}
		}
		self.refresh_local(app);
		if deleted {
			self.tune_focus(app);
		}
	}

	/// Play on Move: the station that took a deleted one's place has the
	/// focus now, so it plays, like moving onto it would (focus_station
	/// checks the setting itself).
	fn tune_focus(&self, app: &Rc<RefCell<App>>) {
		let dial = {
			let m = self.model.borrow();
			m.focus().and_then(|f| m.dial(f))
		};
		if let Some((stations, k)) = dial {
			app.borrow_mut().focus_station(&stations, k);
		}
	}
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::BROWSE_TITLE).build();
	// The label first: wxMSW names the choice after it.
	let sort_label = StaticText::builder(&dialog).with_label(s::SORT).build();
	// Full path: `Choice` here is the context menu's enum.
	let sort_choice = wxdragon::widgets::choice::Choice::builder(&dialog).build();
	for f in SortField::ALL {
		sort_choice.append(f.label());
	}
	let sort0 = app.borrow().browse.sort;
	let at = SortField::ALL
		.iter()
		.position(|f| *f == sort0.field)
		.unwrap_or(0);
	sort_choice.set_selection(at as u32);
	let reverse = CheckBox::builder(&dialog).with_label(s::REVERSE).build();
	reverse.set_value(sort0.reverse);
	let sort_row = BoxSizer::builder(Orientation::Horizontal).build();
	sort_row.add(&sort_choice, 1, SizerFlag::Expand | SizerFlag::Right, 5);
	sort_row.add(&reverse, 0, SizerFlag::All, 0);
	// The label first: wxMSW names the tree after it.
	let label = StaticText::builder(&dialog).with_label(s::STATIONS).build();
	let tree = TreeCtrl::builder(&dialog)
		.with_style(
			TreeCtrlStyle::HasButtons
				| TreeCtrlStyle::LinesAtRoot
				| TreeCtrlStyle::HideRoot
				| TreeCtrlStyle::Single,
		)
		.build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(
		&sort_label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add_sizer(&sort_row, 0, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(
		&label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&tree, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(&close, 0, SizerFlag::All, 5);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(640, 560));

	let Some(root) = tree.add_root("", None, None) else {
		dialog.destroy();
		return None;
	};
	let memory = std::mem::take(&mut app.borrow_mut().browse);
	let mut model = BrowseModel::new(memory, &app.borrow().library);
	model.set_order(
		app.borrow()
			.settings
			.browse_order
			.clone()
			.into_iter()
			.collect(),
	);
	let view = Rc::new(View {
		tree,
		root,
		items: RefCell::new(HashMap::new()),
		model: RefCell::new(model),
		quiet: Cell::new(false),
		sort: Cell::new(sort0),
	});
	let read_sort = Rc::new(move || Sort {
		field: SortField::ALL[sort_choice.get_selection().unwrap_or(0) as usize],
		reverse: reverse.get_value(),
	});
	let (v, read) = (view.clone(), read_sort.clone());
	sort_choice.on_selection_changed(move |_| v.resort(read()));
	let v = view.clone();
	reverse.on_toggled(move |_| v.resort(read_sort()));
	view.sync_deep(None);
	view.expand_open(None);
	view.show_focus();
	let token = WindowToken::new();
	bind(&view, app, dialog, &token);

	tree.set_focus();
	// Landing on a station plays it.
	let focus = view.model.borrow().focus();
	if let Some(f) = focus {
		view.focused(app, f);
	}
	let (v, a) = (view.clone(), app.clone());
	let next = crate::dialogs::run_with_refresh(
		app,
		dialog,
		Action::OpenBrowse,
		tree,
		&token,
		move || v.refresh_local(&a),
	);
	app.borrow_mut().browse = view.model.borrow_mut().memory(view.sort.get());
	next
}

fn bind(view: &Rc<View>, app: &Rc<RefCell<App>>, dialog: Dialog, token: &WindowToken) {
	let tree = view.tree;

	let (v, a) = (view.clone(), app.clone());
	tree.on_selection_changed(move |e| {
		if v.quiet.get() {
			return;
		}
		if let Some(i) = e.get_item().and_then(|item| v.index(&item)) {
			v.focused(&a, i);
		}
	});

	// Right, a click on the button: children load here.
	let (v, a, tok) = (view.clone(), app.clone(), token.clone());
	tree.on_item_expanding(move |e| {
		if v.quiet.get() {
			return;
		}
		if let Some(i) = e.get_item().and_then(|item| v.index(&item)) {
			v.open(&a, &tok, i);
		}
	});

	let v = view.clone();
	tree.on_item_collapsed(move |e| {
		if v.quiet.get() {
			return;
		}
		if let Some(i) = e.get_item().and_then(|item| v.index(&item)) {
			v.model.borrow_mut().close(i);
		}
	});

	let (v, a, tok) = (view.clone(), app.clone(), token.clone());
	tree.on_item_activated(move |e| {
		// Handled: a double click must not toggle the folder as well.
		e.event.skip(false);
		if let Some(i) = e.get_item().and_then(|item| v.index(&item)) {
			v.activate(&a, &tok, dialog, i);
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.bind_internal(EventType::CHAR, move |e: Event| {
		let c = e
			.get_unicode_key()
			.and_then(|k| char::from_u32(k as u32))
			.filter(|c| !c.is_control() && *c != ' ');
		match c.filter(|_| !e.control_down() && !e.alt_down()) {
			// Consumed: the native type-ahead would jump as well.
			Some(c) => {
				e.skip(false);
				let to = v.model.borrow().jump(c);
				if let Some(to) = to {
					v.go(&a, to);
				}
			}
			None => e.skip(true),
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.bind_internal(EventType::KEY_DOWN, move |e: Event| {
		let (Some(code), Some(f)) = (e.get_key_code(), v.model.borrow().focus()) else {
			return e.skip(true);
		};
		let ctrl = e.control_down();
		let plain = !(e.alt_down() || e.shift_down());
		// Shift+Up / Shift+Down moves the focused row among its own
		// kind on its level: a folder among folders, a station among
		// stations (Alt+Up / Alt+Down too, as in Collections).
		let move_dir = match (code, ctrl) {
			(WXK_UP, false) if !plain => Some(true),
			(WXK_DOWN, false) if !plain => Some(false),
			_ => None,
		};
		if let Some(up) = move_dir {
			e.skip(false);
			let folder = {
				let m = v.model.borrow();
				m.focus().is_some_and(|f| m.is_folder(f))
			};
			if folder {
				v.move_folder(&a, up);
			} else {
				v.move_station(&a, up);
			}
			return;
		}
		let to = {
			let m = v.model.borrow();
			match (code, ctrl) {
				(WXK_HOME, false) => Some(m.first_on_level(f)),
				(WXK_END, false) => Some(m.last_on_level(f)),
				(WXK_HOME, true) => Some(m.top()),
				(WXK_END, true) => Some(m.bottom()),
				_ => None,
			}
		};
		match (code, to) {
			(_, Some(to)) if plain => {
				e.skip(false);
				if to != f {
					v.go(&a, to);
				}
			}
			(WXK_DELETE, _) if plain && !ctrl => {
				e.skip(false);
				v.delete(&a, f);
			}
			_ => e.skip(true),
		}
	});

	// The Applications key, Shift+F10 and a right click.
	let (v, a, tok) = (view.clone(), app.clone(), token.clone());
	tree.bind_internal(EventType::CONTEXT_MENU, move |e: Event| {
		e.skip(false);
		v.context_menu(&a, &tok);
	});
}
