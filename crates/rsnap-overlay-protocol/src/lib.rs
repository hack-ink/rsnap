use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
	pub x: i32,
	pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
	pub x: i32,
	pub y: i32,
	pub width: u32,
	pub height: u32,
}
impl Rect {
	#[must_use]
	pub fn from_points(a: Point, b: Point) -> Self {
		let (min_x, max_x) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
		let (min_y, max_y) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
		let width = max_x.saturating_sub(min_x) as u32;
		let height = max_y.saturating_sub(min_y) as u32;

		Self { x: min_x, y: min_y, width, height }
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverlayOutput {
	Cancel,
	Region { rect: Rect },
	Window { window_id: u32 },
	Error { message: String },
}

#[cfg(test)]
mod tests {
	use crate::{OverlayOutput, Point, Rect};

	#[test]
	fn rect_from_points_is_normalized() {
		let a = Point { x: 10, y: 20 };
		let b = Point { x: 5, y: 15 };
		let rect = Rect::from_points(a, b);

		assert_eq!(rect, Rect { x: 5, y: 15, width: 5, height: 5 });
	}

	#[test]
	fn json_cancel_is_tagged() {
		let json = serde_json::to_string(&OverlayOutput::Cancel).unwrap();

		assert_eq!(json, r#"{"type":"cancel"}"#);
	}

	#[test]
	fn json_region_is_tagged() {
		let json = serde_json::to_string(&OverlayOutput::Region {
			rect: Rect { x: 1, y: 2, width: 3, height: 4 },
		})
		.unwrap();

		assert_eq!(json, r#"{"type":"region","rect":{"x":1,"y":2,"width":3,"height":4}}"#);
	}

	#[test]
	fn json_window_is_tagged() {
		let json = serde_json::to_string(&OverlayOutput::Window { window_id: 12 }).unwrap();

		assert_eq!(json, r#"{"type":"window","window_id":12}"#);
	}

	#[test]
	fn json_error_is_tagged() {
		let json =
			serde_json::to_string(&OverlayOutput::Error { message: String::from("boom") }).unwrap();

		assert_eq!(json, r#"{"type":"error","message":"boom"}"#);
	}
}
