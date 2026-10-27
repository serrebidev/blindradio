use thiserror::Error;

/// Display is the bare inner text; the UI adds any prefix.
#[derive(Debug, Error)]
pub enum CoreError {
	#[error("{0}")]
	Http(String),
	#[error("{0}")]
	Playlist(String),
	#[error("{0}")]
	Decode(String),
	#[error("{0}")]
	Output(String),
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn error_displays_terse() {
		assert_eq!(CoreError::Http("timeout".into()).to_string(), "timeout");
	}
}
