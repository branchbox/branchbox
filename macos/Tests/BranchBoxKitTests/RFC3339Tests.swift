import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

/// A UTC instant built with `Calendar`, independent of the parser under test.
func utc(_ year: Int, _ month: Int, _ day: Int, _ hour: Int = 0, _ minute: Int = 0, _ second: Int = 0,
         nanoseconds: Int = 0) -> Date {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "UTC")!
    let components = DateComponents(year: year, month: month, day: day, hour: hour, minute: minute, second: second)
    return calendar.date(from: components)!.addingTimeInterval(Double(nanoseconds) / 1_000_000_000)
}

/// Equal to within a microsecond (a `Date` near 2026 resolves to about 0.2 µs).
func sameInstant(_ lhs: Date?, _ rhs: Date) -> Bool {
    guard let lhs else { return false }
    return abs(lhs.timeIntervalSince1970 - rhs.timeIntervalSince1970) < 0.000_001
}

struct RFC3339Case: Sendable, CustomTestStringConvertible {
    let text: String
    let expected: Date
    var testDescription: String { text }
}

@Suite struct RFC3339Tests {
    static let valid: [RFC3339Case] = [
        // Whole seconds (no fraction digits), as core writes `2026-09-30T10:00:00Z`.
        RFC3339Case(text: "2026-09-30T10:00:00Z", expected: utc(2026, 9, 30, 10)),
        // 1 digit.
        RFC3339Case(text: "2026-09-30T10:05:00.5Z", expected: utc(2026, 9, 30, 10, 5, 0, nanoseconds: 500_000_000)),
        // 6 digits: the registry's chrono microseconds.
        RFC3339Case(text: "2026-03-17T03:37:57.979509Z", expected: utc(2026, 3, 17, 3, 37, 57, nanoseconds: 979_509_000)),
        // 9 digits: nanoseconds from older registries.
        RFC3339Case(text: "2025-11-10T04:29:00.768535795Z",
                    expected: utc(2025, 11, 10, 4, 29, 0, nanoseconds: 768_535_795)),
        RFC3339Case(text: "2026-09-30T10:00:00.123456789Z",
                    expected: utc(2026, 9, 30, 10, 0, 0, nanoseconds: 123_456_789)),
        // `+00:00`: `generated_at` in start summaries.
        RFC3339Case(text: "2026-10-01T22:51:10.903377+00:00",
                    expected: utc(2026, 10, 1, 22, 51, 10, nanoseconds: 903_377_000)),
        // Negative and positive offsets move the instant to UTC.
        RFC3339Case(text: "2026-10-01T18:51:10-04:00", expected: utc(2026, 10, 1, 22, 51, 10)),
        RFC3339Case(text: "2026-10-02T04:21:10+05:30", expected: utc(2026, 10, 1, 22, 51, 10)),
        // Offsets crossing midnight and a year boundary.
        RFC3339Case(text: "2025-12-31T23:30:00-01:00", expected: utc(2026, 1, 1, 0, 30)),
        // Leap day, before the epoch, and RFC 3339's lowercase/space separators.
        RFC3339Case(text: "2024-02-29T12:00:00Z", expected: utc(2024, 2, 29, 12)),
        RFC3339Case(text: "1969-12-31T23:59:59Z", expected: utc(1969, 12, 31, 23, 59, 59)),
        RFC3339Case(text: "2026-09-30t10:00:00z", expected: utc(2026, 9, 30, 10)),
        RFC3339Case(text: "2026-09-30 10:00:00Z", expected: utc(2026, 9, 30, 10)),
    ]

    @Test(arguments: RFC3339Tests.valid)
    func parsesValidTimestamps(_ testCase: RFC3339Case) {
        #expect(sameInstant(RFC3339.parse(testCase.text), testCase.expected))
    }

    @Test func wholeSecondsAndFractionsAgree() {
        #expect(RFC3339.parse("2026-09-30T10:00:00Z") == RFC3339.parse("2026-09-30T10:00:00.000000Z"))
        #expect(RFC3339.parse("2026-09-30T10:00:00Z") == RFC3339.parse("2026-09-30T10:00:00+00:00"))
    }

    @Test func ignoresDigitsPastNanoseconds() {
        #expect(sameInstant(RFC3339.parse("2026-09-30T10:00:00.1234567891234Z"),
                            utc(2026, 9, 30, 10, 0, 0, nanoseconds: 123_456_789)))
    }

    @Test(arguments: [
        "", "garbage", "2026", "null",
        "2026-03-17", "2026-03-17T03:37", "2026-03-17T03:37:57",          // truncated / no offset
        "2026-3-17T03:37:57Z", "26-03-17T03:37:57Z",                       // short fields
        "2026-13-01T00:00:00Z", "2026-00-10T00:00:00Z",                    // month out of range
        "2026-02-30T00:00:00Z", "2023-02-29T00:00:00Z", "2026-04-31T00:00:00Z", // no such day
        "2026-03-17T24:00:00Z", "2026-03-17T03:60:00Z", "2026-03-17T03:37:61Z", // time out of range
        "2026-03-17T03:37:57.Z", "2026-03-17T03:37:57.5",                  // empty fraction / no offset
        "2026-03-17T03:37:57+0400", "2026-03-17T03:37:57+04", "2026-03-17T03:37:57+24:00",
        "2026-03-17T03:37:57Zjunk", " 2026-03-17T03:37:57Z", "2026-03-17T03:37:57Z ",
        "2026-03-17X03:37:57Z", "2026/03/17T03:37:57Z", "１２３４-03-17T03:37:57Z",
    ])
    func rejectsGarbage(_ text: String) {
        #expect(RFC3339.parse(text) == nil)
    }

    @Test(arguments: RFC3339Tests.valid)
    func formatRoundTrips(_ testCase: RFC3339Case) throws {
        let date = try #require(RFC3339.parse(testCase.text))
        let formatted = RFC3339.format(date)
        #expect(formatted.hasSuffix("Z"))
        #expect(sameInstant(RFC3339.parse(formatted), date))
    }

    @Test func formatsMicrosecondsInUTC() {
        #expect(RFC3339.format(utc(2026, 3, 17, 3, 37, 57, nanoseconds: 979_509_000)) == "2026-03-17T03:37:57.979509Z")
        #expect(RFC3339.format(utc(1969, 12, 31, 23, 59, 59)) == "1969-12-31T23:59:59.000000Z")
        #expect(RFC3339.format(utc(2024, 2, 29)) == "2024-02-29T00:00:00.000000Z")
    }

    @Test func everyCapturedRecordDateParses() throws {
        let data = try Fixtures.data("cli-0.13.4/main_feature_list_all.json")
        let raw = try #require(try JSONSerialization.jsonObject(with: data) as? [[String: Any]])
        for record in raw {
            for key in ["created_at", "updated_at", "removed_at"] {
                guard let text = record[key] as? String else { continue }
                #expect(RFC3339.parse(text) != nil, "\(key) = \(text)")
            }
        }
    }
}
