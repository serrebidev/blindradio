use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Station {
	pub id: String,
	pub name: String,
	pub url: String,
}
