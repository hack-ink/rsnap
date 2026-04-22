// swift-tools-version: 6.0

import Foundation
import PackageDescription

let packageRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let repoRoot = packageRoot.deletingLastPathComponent().deletingLastPathComponent()
let defaultRustLibDir = repoRoot.appendingPathComponent("target/debug").path
let rustLibDir = ProcessInfo.processInfo.environment["RSNAP_HOST_FFI_LIB_DIR"] ?? defaultRustLibDir

let package = Package(
	name: "RsnapNativeHost",
	platforms: [
		.macOS(.v14),
	],
	products: [
		.library(name: "RsnapHostBridge", targets: ["RsnapHostBridge"]),
		.library(name: "RsnapNativeHostKit", targets: ["RsnapNativeHostKit"]),
		.executable(name: "RsnapHostBridgeProbe", targets: ["RsnapHostBridgeProbe"]),
		.executable(name: "RsnapNativeHost", targets: ["RsnapNativeHost"]),
	],
	targets: [
		.systemLibrary(
			name: "CRsnapHostFFI",
			path: "Sources/CRsnapHostFFI"
		),
		.target(
			name: "RsnapHostBridge",
			dependencies: ["CRsnapHostFFI"],
			linkerSettings: [
				.linkedFramework("AppKit"),
				.linkedFramework("Carbon"),
				.linkedFramework("CoreGraphics"),
				.linkedFramework("CoreMedia"),
				.linkedFramework("CoreVideo"),
				.linkedFramework("Metal"),
				.linkedFramework("QuartzCore"),
				.linkedFramework("ScreenCaptureKit"),
				.linkedFramework("Vision"),
				.unsafeFlags([
					"-L",
					rustLibDir,
					"-lrsnap_host_ffi",
				]),
			]
		),
		.target(
			name: "RsnapNativeHostKit",
			dependencies: ["RsnapHostBridge"],
			linkerSettings: [
				.linkedFramework("AppKit"),
				.linkedFramework("Carbon"),
				.linkedFramework("CoreMedia"),
				.linkedFramework("CoreVideo"),
				.linkedFramework("ScreenCaptureKit"),
				.linkedFramework("Vision"),
			]
		),
		.executableTarget(
			name: "RsnapHostBridgeProbe",
			dependencies: ["RsnapHostBridge"]
		),
		.executableTarget(
			name: "RsnapNativeHost",
			dependencies: ["RsnapNativeHostKit"]
		),
	]
)
