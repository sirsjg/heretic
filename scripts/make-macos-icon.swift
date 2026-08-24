#!/usr/bin/env swift
//
// Build the macOS app icon from the square source artwork.
//
// macOS does not round app icons for you: the .icns has to ship with the
// corners already cut. Apple's grid puts the artwork in an 824x824 rounded
// square centred on a 1024x1024 canvas, with the continuous ("squircle")
// corner curve every system app uses. Tauri would otherwise generate a plain
// square .icns straight from icons/icon.png, which is why the dock icon looked
// like a tile.
//
// Run it after changing the artwork:
//
//   scripts/make-macos-icon.swift
//
// It reads crates/heretic-app/icons/icon-1024.png and writes
// crates/heretic-app/icons/icon.icns.

import AppKit

let fm = FileManager.default
let root = URL(fileURLWithPath: fm.currentDirectoryPath)
let iconsDir = root.appendingPathComponent("crates/heretic-app/icons")
let source = iconsDir.appendingPathComponent("icon-1024.png")
let output = iconsDir.appendingPathComponent("icon.icns")

// Apple's macOS icon grid, expressed as fractions of the canvas.
let contentScale = 824.0 / 1024.0
let cornerScale = 185.4 / 824.0

guard let imageSource = CGImageSourceCreateWithURL(source as CFURL, nil),
      let artwork = CGImageSourceCreateImageAtIndex(imageSource, 0, nil)
else {
    FileHandle.standardError.write("cannot read \(source.path)\n".data(using: .utf8)!)
    exit(1)
}

let colorSpace = CGColorSpaceCreateDeviceRGB()
let bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue

func context(_ side: Int) -> CGContext {
    guard let ctx = CGContext(
        data: nil,
        width: side,
        height: side,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: colorSpace,
        bitmapInfo: bitmapInfo
    ) else { fatalError("cannot allocate a \(side)x\(side) context") }
    return ctx
}

/// A canvas-sized image whose only opaque region is the rounded square the
/// artwork is allowed to fill.
func squircleMask(side: Int) -> CGImage {
    let canvas = CGFloat(side)
    let content = (canvas * contentScale).rounded()
    let inset = ((canvas - content) / 2).rounded()

    let layer = CALayer()
    layer.frame = CGRect(x: 0, y: 0, width: content, height: content)
    layer.backgroundColor = NSColor.white.cgColor
    layer.cornerRadius = content * cornerScale
    layer.cornerCurve = .continuous
    layer.masksToBounds = true

    let ctx = context(side)
    ctx.translateBy(x: inset, y: inset)
    layer.render(in: ctx)
    return ctx.makeImage()!
}

func icon(side: Int) -> CGImage {
    let canvas = CGFloat(side)
    let content = (canvas * contentScale).rounded()
    let inset = ((canvas - content) / 2).rounded()

    let ctx = context(side)
    ctx.interpolationQuality = .high
    ctx.draw(artwork, in: CGRect(x: inset, y: inset, width: content, height: content))
    // Cut the corners out of what we just drew.
    ctx.setBlendMode(.destinationIn)
    ctx.draw(squircleMask(side: side), in: CGRect(x: 0, y: 0, width: canvas, height: canvas))
    return ctx.makeImage()!
}

func writePNG(_ image: CGImage, to url: URL) {
    guard let dest = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil)
    else { fatalError("cannot write \(url.path)") }
    CGImageDestinationAddImage(dest, image, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("cannot finalize \(url.path)") }
}

let iconset = fm.temporaryDirectory.appendingPathComponent("heretic-\(getpid()).iconset")
try? fm.removeItem(at: iconset)
try fm.createDirectory(at: iconset, withIntermediateDirectories: true)
defer { try? fm.removeItem(at: iconset) }

// iconutil wants every @1x/@2x pair present, so each pixel size gets rendered
// once and linked under both names it is known by.
let names: [Int: [String]] = [
    16: ["icon_16x16.png"],
    32: ["icon_16x16@2x.png", "icon_32x32.png"],
    64: ["icon_32x32@2x.png"],
    128: ["icon_128x128.png"],
    256: ["icon_128x128@2x.png", "icon_256x256.png"],
    512: ["icon_256x256@2x.png", "icon_512x512.png"],
    1024: ["icon_512x512@2x.png"],
]

for (side, files) in names.sorted(by: { $0.key < $1.key }) {
    let rendered = icon(side: side)
    for file in files {
        writePNG(rendered, to: iconset.appendingPathComponent(file))
    }
}

let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["--convert", "icns", iconset.path, "--output", output.path]
try iconutil.run()
iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else { exit(iconutil.terminationStatus) }

print("wrote \(output.path)")
