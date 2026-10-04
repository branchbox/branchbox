@testable import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

@Suite struct LineSplitterTests {
    @Test func splitsOnLineFeedCRLFAndBareCarriageReturn() {
        var splitter = LineSplitter()

        #expect(splitter.append(Data("a\nb\r\nc\rd".utf8)) == ["a", "b", "c"])
        #expect(splitter.flush() == "d")
        #expect(splitter.flush() == nil)
    }

    @Test func crlfSplitAcrossChunksIsOneTerminator() {
        var splitter = LineSplitter()

        #expect(splitter.append(Data("a\r".utf8)) == ["a"])
        #expect(splitter.append(Data("\nb\n".utf8)) == ["b"])
        #expect(splitter.append(Data("c\r".utf8)) == ["c"])
        #expect(splitter.append(Data("d\n".utf8)) == ["d"])
    }

    @Test func blankLinesArePreserved() {
        var splitter = LineSplitter()

        #expect(splitter.append(Data("Error: x\n\nCaused by:\n".utf8)) == ["Error: x", "", "Caused by:"])
    }

    @Test func multiByteCharactersSurviveAnyChunking() {
        let bytes = Array("héllo ✓ wörld\nnext 🚀\n".utf8)
        for cut in 1..<bytes.count {
            var splitter = LineSplitter()
            let lines = splitter.append(Data(bytes[..<cut])) + splitter.append(Data(bytes[cut...]))
            #expect(lines == ["héllo ✓ wörld", "next 🚀"], "cut at byte \(cut)")
        }
    }

    @Test func longLinesAreCutAtACharacterBoundaryAndMarked() {
        var splitter = LineSplitter(maxLineBytes: 16)

        #expect(splitter.append(Data("abcdefghijklmnopqrstuvwxyz\nnext\n".utf8)) == ["abcdefghijklm…", "next"])
        // 9 two-byte characters: 13 bytes of room would split the 7th, so 6 are kept.
        let accented = splitter.append(Data("ééééééééé\n".utf8))
        #expect(accented == ["éééééé…"])
        #expect(accented.allSatisfy { $0.utf8.count <= 16 })
    }

    @Test func longLinesStayCappedAcrossChunksAndAtFlush() throws {
        var splitter = LineSplitter()
        let chunk = Data(repeating: UInt8(ascii: "x"), count: 10_000)

        #expect(splitter.append(chunk).isEmpty)
        #expect(splitter.append(chunk).isEmpty)
        let flushed = splitter.flush()
        let last = try #require(flushed)

        #expect(last.utf8.count == LogLine.maxMessageBytes)
        #expect(last.hasSuffix("…"))
    }
}

@Suite struct ANSITests {
    @Test(arguments: [
        ("\u{1B}[2m2026-10-01T22:50:58Z\u{1B}[0m \u{1B}[32m INFO\u{1B}[0m git", "2026-10-01T22:50:58Z  INFO git"),
        ("\u{1B}[?25lhidden cursor\u{1B}[?25h", "hidden cursor"),
        ("\u{1B}[2K\u{1B}[1Gredrawn", "redrawn"),
        ("\u{1B}]8;;https://example.com\u{07}link\u{1B}]8;;\u{07}", "link"),
        ("\u{1B}]0;title\u{1B}\\after", "after"),
        ("\u{1B}(Bcharset", "charset"),
        ("\u{1B}7saved\u{1B}8", "saved"),
        ("trailing escape\u{1B}", "trailing escape"),
        ("\u{1B}[é kept", "é kept"),
        ("plain ✓ text", "plain ✓ text"),
    ])
    func strips(_ input: String, _ expected: String) {
        #expect(ANSI.strip(input) == expected)
    }
}

@Suite struct LineTailTests {
    @Test func keepsTheLastLinesInOrder() {
        var tail = LineTail(limit: 3)
        for line in ["1", "2", "3", "4", "5"] { tail.append(line) }

        #expect(tail.lines == ["3", "4", "5"])
    }

    @Test func keepsEverythingBelowTheLimitAndNothingAtZero() {
        var short = LineTail(limit: 5)
        short.append("a")
        short.append("b")
        var none = LineTail(limit: 0)
        none.append("a")

        #expect(short.lines == ["a", "b"])
        #expect(none.lines.isEmpty)
    }
}
