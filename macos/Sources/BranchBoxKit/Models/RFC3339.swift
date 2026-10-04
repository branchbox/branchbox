import Foundation

/// RFC 3339 timestamps as the CLI prints them (chrono): `YYYY-MM-DDTHH:MM:SS[.f…](Z|±HH:MM)`.
///
/// Hand-rolled so every macOS release parses the same way: `ISO8601DateFormatter` and the
/// `.iso8601` decoding strategy differ in how they treat fractional seconds across OS versions.
public enum RFC3339 {
    /// Parses 0–9 (or more; digits past nanoseconds are ignored) fraction digits and a `Z` or
    /// `±HH:MM` offset. Returns nil for anything else, including impossible dates such as Feb 30.
    public static func parse(_ text: String) -> Date? {
        let bytes = Array(text.utf8)
        var index = 0

        func digits(_ count: Int) -> Int? {
            guard index + count <= bytes.count else { return nil }
            var value = 0
            for offset in 0..<count {
                let byte = bytes[index + offset]
                guard byte >= UInt8(ascii: "0"), byte <= UInt8(ascii: "9") else { return nil }
                value = value * 10 + Int(byte - UInt8(ascii: "0"))
            }
            index += count
            return value
        }

        func literal(_ accepted: UInt8...) -> Bool {
            guard index < bytes.count, accepted.contains(bytes[index]) else { return false }
            index += 1
            return true
        }

        guard let year = digits(4), literal(UInt8(ascii: "-")),
              let month = digits(2), literal(UInt8(ascii: "-")),
              let day = digits(2), literal(UInt8(ascii: "T"), UInt8(ascii: "t"), UInt8(ascii: " ")),
              let hour = digits(2), literal(UInt8(ascii: ":")),
              let minute = digits(2), literal(UInt8(ascii: ":")),
              let second = digits(2),
              (1...12).contains(month), (1...daysInMonth(month, year: year)).contains(day),
              hour < 24, minute < 60, second <= 60 else { return nil }

        var nanoseconds = 0
        if literal(UInt8(ascii: ".")) {
            var count = 0
            while index < bytes.count, bytes[index] >= UInt8(ascii: "0"), bytes[index] <= UInt8(ascii: "9") {
                if count < 9 { nanoseconds = nanoseconds * 10 + Int(bytes[index] - UInt8(ascii: "0")) }
                count += 1
                index += 1
            }
            guard count > 0 else { return nil }
            for _ in min(count, 9)..<9 { nanoseconds *= 10 }
        }

        var offsetSeconds = 0
        if literal(UInt8(ascii: "Z"), UInt8(ascii: "z")) {
            offsetSeconds = 0
        } else if index < bytes.count, bytes[index] == UInt8(ascii: "+") || bytes[index] == UInt8(ascii: "-") {
            let sign = bytes[index] == UInt8(ascii: "-") ? -1 : 1
            index += 1
            guard let offsetHours = digits(2), literal(UInt8(ascii: ":")), let offsetMinutes = digits(2),
                  offsetHours < 24, offsetMinutes < 60 else { return nil }
            offsetSeconds = sign * (offsetHours * 3600 + offsetMinutes * 60)
        } else {
            return nil
        }
        guard index == bytes.count else { return nil }

        let wholeSeconds = daysFromCivil(year: year, month: month, day: day) * 86_400
            + hour * 3600 + minute * 60 + second - offsetSeconds
        return Date(timeIntervalSince1970: Double(wholeSeconds) + Double(nanoseconds) / 1_000_000_000)
    }

    /// Formats in UTC with microseconds (`2026-03-17T03:37:57.979509Z`), the shape `parse` reads back.
    public static func format(_ date: Date) -> String {
        let interval = date.timeIntervalSince1970
        var wholeSeconds = Int(interval.rounded(.down))
        var microseconds = Int(((interval - Double(wholeSeconds)) * 1_000_000).rounded())
        if microseconds >= 1_000_000 {
            wholeSeconds += 1
            microseconds -= 1_000_000
        }
        let days = Int((Double(wholeSeconds) / 86_400).rounded(.down))
        let secondOfDay = wholeSeconds - days * 86_400
        let (year, month, day) = civilFromDays(days)
        return String(format: "%04ld-%02ld-%02ldT%02ld:%02ld:%02ld.%06ldZ", year, month, day,
                      secondOfDay / 3600, secondOfDay % 3600 / 60, secondOfDay % 60, microseconds)
    }

    private static func daysInMonth(_ month: Int, year: Int) -> Int {
        switch month {
        case 2: (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 ? 29 : 28
        case 4, 6, 9, 11: 30
        default: 31
        }
    }

    /// Howard Hinnant's `days_from_civil`: days since 1970-01-01 in the proleptic Gregorian calendar.
    private static func daysFromCivil(year: Int, month: Int, day: Int) -> Int {
        let y = month <= 2 ? year - 1 : year
        let era = (y >= 0 ? y : y - 399) / 400
        let yearOfEra = y - era * 400
        let monthIndex = (month + 9) % 12
        let dayOfYear = (153 * monthIndex + 2) / 5 + day - 1
        let dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear
        return era * 146_097 + dayOfEra - 719_468
    }

    /// The inverse of `daysFromCivil` (Hinnant's `civil_from_days`).
    private static func civilFromDays(_ days: Int) -> (year: Int, month: Int, day: Int) {
        let z = days + 719_468
        let era = (z >= 0 ? z : z - 146_096) / 146_097
        let dayOfEra = z - era * 146_097
        let yearOfEra = (dayOfEra - dayOfEra / 1460 + dayOfEra / 36_524 - dayOfEra / 146_096) / 365
        let dayOfYear = dayOfEra - (365 * yearOfEra + yearOfEra / 4 - yearOfEra / 100)
        let monthIndex = (5 * dayOfYear + 2) / 153
        let day = dayOfYear - (153 * monthIndex + 2) / 5 + 1
        let month = monthIndex < 10 ? monthIndex + 3 : monthIndex - 9
        return (yearOfEra + era * 400 + (month <= 2 ? 1 : 0), month, day)
    }
}

extension KeyedDecodingContainer {
    /// R-5: a date is a string parsed by `RFC3339.parse`; absent, null, non-string or bad → nil.
    func rfc3339IfPresent(forKey key: Key) -> Date? {
        lenient(String.self, forKey: key).flatMap(RFC3339.parse)
    }
}

extension KeyedEncodingContainer {
    /// Writes a date back in the CLI's own RFC 3339 form so `CLIJSON` can decode it again.
    mutating func encodeRFC3339IfPresent(_ date: Date?, forKey key: Key) throws {
        try encodeIfPresent(date.map(RFC3339.format), forKey: key)
    }
}
