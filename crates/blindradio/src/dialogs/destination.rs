//! Native folder picker shared by station creation and filing menus.

use blindradio_core::library::{Folder, Library, StationDest};
use wx_utils::{add_ok_cancel_footer, build_ok_cancel_buttons};
use wxdragon::prelude::*;

use crate::strings as s;

pub struct DestinationTree {
	pub label: StaticText,
	pub tree: TreeCtrl,
	rows: Vec<(TreeItemId, StationDest)>,
	allowed: Vec<StationDest>,
}

fn path(dest: &StationDest) -> &[usize] {
	match dest {
		StationDest::Favorites(path) | StationDest::Collection { path, .. } => path,
	}
}

fn within(parent: &StationDest, child: &StationDest) -> bool {
	let same_list = match (parent, child) {
		(StationDest::Favorites(_), StationDest::Favorites(_)) => true,
		(StationDest::Collection { id: a, .. }, StationDest::Collection { id: b, .. }) => a == b,
		_ => false,
	};
	same_list && path(child).starts_with(path(parent))
}

fn at(parent: &StationDest, path: Vec<usize>) -> StationDest {
	match parent {
		StationDest::Favorites(_) => StationDest::Favorites(path),
		StationDest::Collection { id, .. } => StationDest::Collection {
			id: id.clone(),
			path,
		},
	}
}

impl DestinationTree {
	pub fn new(parent: &impl WxWidget, lib: &Library, allowed: Vec<StationDest>) -> Self {
		let label = StaticText::builder(parent)
			.with_label(&s::destination())
			.build();
		let tree = TreeCtrl::builder(parent)
			.with_style(
				TreeCtrlStyle::HasButtons
					| TreeCtrlStyle::LinesAtRoot
					| TreeCtrlStyle::HideRoot
					| TreeCtrlStyle::Single,
			)
			.with_size(Size::new(420, 240))
			.build();
		let root = tree.add_root("", None, None).expect("destination root");
		let mut picker = Self {
			label,
			tree,
			rows: Vec::new(),
			allowed,
		};
		for (name, dest) in lib
			.destinations()
			.into_iter()
			.filter(|(_, d)| path(d).is_empty())
		{
			let folders = match &dest {
				StationDest::Favorites(_) => lib.fav_folders.as_slice(),
				StationDest::Collection { id, .. } => lib
					.collections
					.iter()
					.find(|c| c.id == *id)
					.map_or(&[][..], |c| c.folders.as_slice()),
			};
			picker.append(&root, &name, dest, folders);
		}
		if let Some((item, _)) = picker.rows.iter().find(|(_, d)| picker.allowed.contains(d)) {
			picker.tree.select_item(item);
			picker.tree.ensure_visible(item);
		}
		picker
	}

	fn append(&mut self, parent: &TreeItemId, name: &str, dest: StationDest, folders: &[Folder]) {
		if !self.allowed.iter().any(|d| within(&dest, d)) {
			return;
		}
		let Some(item) = self.tree.append_item(parent, name, None, None) else {
			return;
		};
		self.tree.set_custom_data_direct(&item, dest.clone());
		self.rows.push((item.clone(), dest.clone()));
		for (i, folder) in folders.iter().enumerate() {
			let mut child_path = path(&dest).to_vec();
			child_path.push(i);
			self.append(&item, &folder.name, at(&dest, child_path), &folder.folders);
		}
	}

	pub fn selected(&self) -> Option<StationDest> {
		let item = self.tree.get_selection()?;
		let data = self.tree.get_custom_data_direct(&item)?;
		let dest = data.downcast_ref::<StationDest>()?;
		self.allowed.contains(dest).then(|| dest.clone())
	}

	/// Enter confirms; double-click keeps the tree's native expand/collapse behavior.
	pub fn on_confirm(&self, confirm: impl Fn() + 'static) {
		self.tree.on_key_down(move |e| {
			let WindowEventData::Keyboard(key) = e else {
				return;
			};
			if key.get_key_code() == Some(WXK_RETURN) {
				key.event.skip(false);
				confirm();
			} else {
				key.event.skip(true);
			}
		});
	}
}

pub fn choose(
	parent: &dyn WxWidget,
	lib: &Library,
	title: &str,
	allowed: Vec<StationDest>,
) -> Option<StationDest> {
	let dialog = Dialog::builder(parent, title).build();
	let picker = std::rc::Rc::new(DestinationTree::new(&dialog, lib, allowed));
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&picker.label, 0, SizerFlag::All, 5);
	sizer.add(&picker.tree, 1, SizerFlag::Expand | SizerFlag::All, 5);
	let (ok, cancel) = build_ok_cancel_buttons(&dialog, &s::ok());
	ok.enable(picker.selected().is_some());
	{
		let p = picker.clone();
		picker
			.tree
			.on_selection_changed(move |_| ok.enable(p.selected().is_some()));
	}
	{
		let p = picker.clone();
		picker.on_confirm(move || {
			if p.selected().is_some() {
				dialog.end_modal(ID_OK);
			}
		});
	}
	add_ok_cancel_footer(sizer, ok, cancel);
	dialog.set_sizer_and_fit(sizer, true);
	picker.tree.set_focus();
	let result = (dialog.show_modal() == ID_OK)
		.then(|| picker.selected())
		.flatten();
	dialog.destroy();
	crate::speech::focus_returning();
	result
}
