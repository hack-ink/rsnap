import AppKit
import Foundation

func fail(_ message: String) -> Never {
	fputs("[smoke] FAIL \(message)\n", stderr)
	exit(1)
}

guard CommandLine.arguments.count == 2 else {
	fail("usage: scroll-export-continuity.swift <png-path>")
}

let path = CommandLine.arguments[1]
guard
	let image = NSImage(contentsOfFile: path),
	let tiffData = image.tiffRepresentation,
	let bitmap = NSBitmapImageRep(data: tiffData)
else {
	fail("could not read export image at \(path)")
}

let width = bitmap.pixelsWide
let height = bitmap.pixelsHigh
guard width > 0, height > 0 else {
	fail("export image has invalid dimensions \(width)x\(height)")
}

struct RGB {
	let red: Double
	let green: Double
	let blue: Double
}

func deviceRGB(_ color: NSColor) -> RGB {
	guard let rgb = color.usingColorSpace(.deviceRGB) else {
		return RGB(red: 0, green: 0, blue: 0)
	}

	return RGB(
		red: rgb.redComponent * 255.0,
		green: rgb.greenComponent * 255.0,
		blue: rgb.blueComponent * 255.0
	)
}

func pixelRGB(x: Int, y: Int) -> RGB {
	guard let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) else {
		return RGB(red: 0, green: 0, blue: 0)
	}

	return RGB(
		red: color.redComponent * 255.0,
		green: color.greenComponent * 255.0,
		blue: color.blueComponent * 255.0
	)
}

func distance(_ left: RGB, _ right: RGB) -> Double {
	let red = left.red - right.red
	let green = left.green - right.green
	let blue = left.blue - right.blue

	return red * red + green * green + blue * blue
}

let expectedRows: [RGB] = (0..<80).map { row in
	let hue = CGFloat((row * 37) % 360) / 360.0
	return deviceRGB(
		NSColor(calibratedHue: hue, saturation: 0.24, brightness: 0.97, alpha: 1)
	)
}
let sampleXs = Array(
	Set([
		min(max(20, width / 120), width - 1),
		max(0, width - min(max(20, width / 120), width)),
	])
).sorted()

struct Candidate {
	let meanError: Double
	let maxError: Double
	let rowHeight: Int
	let offset: Int
	let startRow: Int
	let bands: Int
}

func score(rowHeight: Int, offset: Int, startRow: Int) -> Candidate? {
	guard offset >= 0, offset < height else {
		return nil
	}
	let bands = max(0, (height - offset) / rowHeight)
	guard bands >= 4 else {
		return nil
	}
	var total = 0.0
	var maxError = 0.0
	var samples = 0

	for band in 0..<bands {
		let y = offset + band * rowHeight
		let expected = expectedRows[(startRow + band) % expectedRows.count]
		for x in sampleXs {
			let error = distance(pixelRGB(x: x, y: y), expected)
			total += error
			maxError = max(maxError, error)
			samples += 1
		}
	}

	return Candidate(
		meanError: total / Double(max(samples, 1)),
		maxError: maxError,
		rowHeight: rowHeight,
		offset: offset,
		startRow: startRow,
		bands: bands
	)
}

var best: Candidate?
for rowHeight in [72, 144] {
	let step = max(2, rowHeight / 36)
	for offset in stride(from: step, to: rowHeight, by: step) {
		for startRow in 0..<expectedRows.count {
			guard let candidate = score(rowHeight: rowHeight, offset: offset, startRow: startRow)
			else {
				continue
			}
			if let currentBest = best {
				guard candidate.meanError < currentBest.meanError else {
					continue
				}
			}
			best = candidate
		}
	}
}

guard let best else {
	fail("could not score scroll export row continuity")
}

let minimumBands = max(6, height / max(best.rowHeight * 2, 1))
guard best.bands >= minimumBands else {
	fail("row continuity coverage too low: bands=\(best.bands) height=\(height)")
}
guard best.meanError <= 1_500 && best.maxError <= 5_000 else {
	fail(
		"row continuity failed: meanError=\(Int(best.meanError)) maxError=\(Int(best.maxError)) rowHeight=\(best.rowHeight) offset=\(best.offset) startRow=\(best.startRow) bands=\(best.bands)"
	)
}

let endRow = (best.startRow + best.bands - 1) % expectedRows.count
print(
	"row_sequence_ok width=\(width) height=\(height) rowHeight=\(best.rowHeight) offset=\(best.offset) startRow=\(best.startRow) endRow=\(endRow) bands=\(best.bands) meanError=\(Int(best.meanError))"
)
