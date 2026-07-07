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
		.executable(name: "RsnapNativeHostKitProbe", targets: ["RsnapNativeHostKitProbe"]),
		.executable(name: "RsnapNativeHost", targets: ["RsnapNativeHost"]),
	],
	dependencies: [
		.package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.9.4"),
		.package(url: "https://github.com/SimplyDanny/SwiftLintPlugins", exact: "0.65.0"),
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
			dependencies: [
				"RsnapHostBridge",
				.product(name: "Sparkle", package: "Sparkle"),
			],
			resources: [
				.process("Resources"),
			],
			linkerSettings: [
				.linkedFramework("AppKit"),
				.linkedFramework("ApplicationServices"),
				.linkedFramework("Carbon"),
				.linkedFramework("CoreMedia"),
				.linkedFramework("CoreVideo"),
				.linkedFramework("ScreenCaptureKit"),
				.linkedFramework("ServiceManagement"),
				.linkedFramework("Vision"),
			]
		),
		.executableTarget(
			name: "RsnapHostBridgeProbe",
			dependencies: ["RsnapHostBridge"]
		),
		.executableTarget(
			name: "RsnapNativeHostKitProbe",
			dependencies: ["RsnapHostBridge", "RsnapNativeHostKit"]
		),
		.executableTarget(
			name: "RsnapNativeHost",
			dependencies: ["RsnapNativeHostKit"]
		),
	]
)
