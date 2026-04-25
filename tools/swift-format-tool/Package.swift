// swift-tools-version: 6.0
import PackageDescription

let package = Package(
	name: "RsnapSwiftFormatTool",
	platforms: [.macOS(.v13)],
	dependencies: [
		.package(
			url: "https://github.com/apple/swift-format.git",
			exact: "603.0.0-prerelease-2026-02-09"
		)
	]
)
