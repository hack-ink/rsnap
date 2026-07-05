use crate::overlay::{MonitorRect, RectPoints};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) struct WindowFreezeCaptureTarget {
	pub(in crate::overlay) monitor: MonitorRect,
	pub(in crate::overlay) window_id: u32,
	pub(in crate::overlay) rect: RectPoints,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::overlay) enum FrozenCaptureWorkerState {
	#[default]
	Idle,
	Armed,
	Inflight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) enum FrozenExportSessionState {
	Pending {
		worker_state: FrozenCaptureWorkerState,
		window_target: Option<WindowFreezeCaptureTarget>,
	},
	Ready,
	Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::overlay) enum FrozenCaptureSessionState {
	#[default]
	Inactive,
	DisplayPending {
		monitor: MonitorRect,
		worker_state: FrozenCaptureWorkerState,
		window_target: Option<WindowFreezeCaptureTarget>,
	},
	DisplayFailed {
		monitor: MonitorRect,
	},
	DisplayReady {
		monitor: MonitorRect,
		export: FrozenExportSessionState,
	},
}
