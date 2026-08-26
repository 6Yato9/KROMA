import CoreGraphics
import Foundation

/// The set of photographs a session has open, copied out of the engine.
///
/// The engine owns the set; this holds what a filmstrip needs to draw it and
/// nothing else. Only one photograph is ever decoded — a 24-megapixel frame is
/// 96 MB of RGBA, so a folder of two hundred would be twenty gigabytes — and
/// the whole reason a strip exists is to make a set navigable without holding
/// it. So what is here is paths, three marks and an index. Never a frame.
///
/// A value type, and `Equatable`, so the store can write it only when it has
/// actually moved rather than telling every observing view to run its body
/// again for a set that is the same set.
public struct Library: Equatable, Sendable {
    public let entries: [LibraryEntry]
    /// Which photograph is the one on screen, or nil when there is no set —
    /// which is a session showing nothing, and also a session showing the
    /// built-in chart, because a chart is not a file and therefore not a set
    /// of one.
    public let current: Int?

    /// No set open. Not the same as a set of none: the engine refuses to open
    /// an empty list, so that no reader of a set ever has to cope with one.
    public static let empty = Library(entries: [], current: nil)

    public init(entries: [LibraryEntry], current: Int?) {
        self.entries = entries
        self.current = current
    }

    public var isEmpty: Bool { entries.isEmpty }
    public var count: Int { entries.count }

    public subscript(index: Int) -> LibraryEntry? {
        entries.indices.contains(index) ? entries[index] : nil
    }
}

/// One photograph of the set.
///
/// The three marks are the ones a strip draws on a frame: whether the edit
/// parked with it has anything in it to undo, whether its decode failed, and
/// whether its thumbnail has arrived. They come across in one call for all
/// three, because a strip asks all three of every visible entry.
public struct LibraryEntry: Identifiable, Equatable, Sendable {
    /// Where this sits in the set, which is what ``SessionStore/focus(_:)``
    /// takes.
    public let index: Int
    public let path: URL
    /// Whether the edit parked with this photograph has anything in it to undo.
    ///
    /// The engine's own answer, and it says nothing about the photograph on
    /// screen: that one's history is in hand rather than parked, so it reads
    /// false until it is switched away from. A photograph that has never been
    /// opened has no parked edit and is untouched.
    public let edited: Bool
    /// Whether the thumbnail worker could not read the file.
    public let failed: Bool
    /// Whether its thumbnail has arrived.
    public let hasThumbnail: Bool

    /// Identity is the index and not the path, because a set opened from a
    /// list of paths is allowed to contain the same file twice and two views
    /// with one identity is a SwiftUI diagnostic rather than a picture.
    public var id: Int { index }

    /// What a strip writes under the frame: the file's own name.
    public var name: String { path.lastPathComponent }

    public init(index: Int, path: URL, edited: Bool, failed: Bool, hasThumbnail: Bool) {
        self.index = index
        self.path = path
        self.edited = edited
        self.failed = failed
        self.hasThumbnail = hasThumbnail
    }
}

/// A thumbnail as the engine hands it over: RGBA, eight bits a channel, rows
/// top to bottom, `pe_session::library::THUMB_EDGE` on its longest side.
///
/// Bytes rather than a picture, because that is what crosses the ABI — a
/// picture belongs to a graphics context and there are two shells with two of
/// those. ``Thumbnail/image`` is this side's inch of the journey.
public struct Thumbnail: Sendable {
    public let width: Int
    public let height: Int
    public let rgba: [UInt8]

    public init(width: Int, height: Int, rgba: [UInt8]) {
        self.width = width
        self.height = height
        self.rgba = rgba
    }

    /// The bytes as something a view can draw, or nil if they are not the
    /// shape they claim to be.
    ///
    /// `noneSkipLast` rather than either alpha: `pe_io::thumbnail` averages the
    /// three colour channels and writes 255 into the fourth, so the last byte
    /// is padding and reading it as coverage — premultiplied or not — would be
    /// reading a number that means nothing.
    ///
    /// The device RGB space, because the engine has already rendered these
    /// through the document's own colour management; asking Core Graphics to
    /// convert them a second time would be a second opinion about a question
    /// that has been answered.
    public var image: CGImage? {
        guard width > 0, height > 0, rgba.count == width * height * 4 else { return nil }
        guard let provider = CGDataProvider(data: Data(rgba) as CFData) else { return nil }
        return CGImage(
            width: width,
            height: height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: width * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        )
    }
}
