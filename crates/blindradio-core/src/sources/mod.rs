//! Station directories and the browse tree they share.

use crate::station::Station;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
	Folder { id: String, name: String },
	Station(Station),
	LoadMore { id: String },
}
