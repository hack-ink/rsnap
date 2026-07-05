/// Result code returned by FFI entry points.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapStatus {
	/// The operation succeeded.
	Ok = 0,
	/// The provided session handle was null.
	NullHandle = 1,
	/// The provided output pointer was null.
	NullOutput = 2,
	/// No queued value was available.
	Empty = 3,
	/// The provided input payload was invalid.
	InvalidInput = 4,
}
