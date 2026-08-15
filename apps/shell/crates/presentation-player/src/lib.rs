// The Presentation runtime is shared by the standalone development binary and the Workspace 1
// host. Keeping one source prevents embedded and standalone playback from drifting; the remaining
// binary-only entry point is harmless inside the library crate.
include!("main.rs");
