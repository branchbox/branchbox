/// Removes terminal escape sequences from CLI output. `NO_COLOR=1` and `TERM=dumb` keep the CLI quiet, but
/// tools it runs (docker, git, devcontainer) may still colour their output.
enum ANSI {
    private static let escape: UInt8 = 0x1B

    /// Removes CSI sequences (`ESC [` parameters, intermediates, one final byte), OSC sequences (`ESC ]` up to
    /// BEL or `ESC \`) and the other `ESC`-introduced sequences. A malformed sequence loses only its
    /// introducer, so the text after it survives.
    static func strip(_ text: String) -> String {
        guard text.utf8.contains(escape) else { return text }
        let bytes = Array(text.utf8)
        var output: [UInt8] = []
        output.reserveCapacity(bytes.count)
        var index = 0
        while index < bytes.count {
            guard bytes[index] == escape else {
                output.append(bytes[index])
                index += 1
                continue
            }
            index += 1
            guard index < bytes.count else { break }
            switch bytes[index] {
            case UInt8(ascii: "["):
                index += 1
                while index < bytes.count, (0x20...0x3F).contains(bytes[index]) { index += 1 }
                if index < bytes.count, (0x40...0x7E).contains(bytes[index]) { index += 1 }
            case UInt8(ascii: "]"):
                index += 1
                while index < bytes.count {
                    if bytes[index] == 0x07 { index += 1; break }
                    if bytes[index] == escape, index + 1 < bytes.count, bytes[index + 1] == UInt8(ascii: "\\") {
                        index += 2
                        break
                    }
                    index += 1
                }
            default:
                // nF and Fp/Fe/Fs escapes: intermediates (0x20–0x2F), then one final byte (0x30–0x7E).
                while index < bytes.count, (0x20...0x2F).contains(bytes[index]) { index += 1 }
                if index < bytes.count, (0x30...0x7E).contains(bytes[index]) { index += 1 }
            }
        }
        return String(decoding: output, as: UTF8.self)
    }
}
