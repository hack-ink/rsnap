use crate::overlay::{GlobalPoint, MonitorRect, Pos2};

pub(in crate::overlay) fn global_to_local(
	cursor: GlobalPoint,
	monitor: MonitorRect,
) -> Option<Pos2> {
	let (x, y) = monitor.local_u32(cursor)?;

	Some(Pos2::new(x as f32, y as f32))
}
