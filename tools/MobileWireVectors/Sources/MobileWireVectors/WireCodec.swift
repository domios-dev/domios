import Foundation

public enum WireCodecError: String, Error, Equatable {
    case duplicateKey = "duplicate_key"
    case malformedJSON = "malformed_json"
    case noncanonicalJSON = "noncanonical_json"
    case invalidTarget = "invalid_target"
    case invalidPercentEncoding = "invalid_percent_encoding"
    case dotSegmentAboveRoot = "dot_segment_above_root"
    case fragmentNotAllowed = "fragment_not_allowed"
    case noncanonicalTarget = "noncanonical_target"
}

public indirect enum JSONNode: Equatable {
    case null
    case bool(Bool)
    case number(String)
    case string(String)
    case array([JSONNode])
    case object([(String, JSONNode)])

    public static func == (lhs: JSONNode, rhs: JSONNode) -> Bool {
        switch (lhs, rhs) {
        case (.null, .null): return true
        case let (.bool(a), .bool(b)): return a == b
        case let (.number(a), .number(b)): return a == b
        case let (.string(a), .string(b)): return a == b
        case let (.array(a), .array(b)): return a == b
        case let (.object(a), .object(b)):
            return a.count == b.count && zip(a, b).allSatisfy { left, right in
                left.0 == right.0 && left.1 == right.1
            }
        default: return false
        }
    }
}

private struct JSONParser {
    let bytes: [UInt8]
    var index = 0

    mutating func parse() throws -> JSONNode {
        skipWhitespace()
        let value = try parseValue()
        skipWhitespace()
        guard index == bytes.count else { throw WireCodecError.malformedJSON }
        return value
    }

    mutating func parseValue() throws -> JSONNode {
        guard index < bytes.count else { throw WireCodecError.malformedJSON }
        switch bytes[index] {
        case 0x6e: try consume("null"); return .null
        case 0x74: try consume("true"); return .bool(true)
        case 0x66: try consume("false"); return .bool(false)
        case 0x22: return .string(try parseString())
        case 0x5b: return try parseArray()
        case 0x7b: return try parseObject()
        case 0x2d, 0x30...0x39: return .number(try parseNumber())
        default: throw WireCodecError.malformedJSON
        }
    }

    mutating func parseArray() throws -> JSONNode {
        index += 1
        skipWhitespace()
        var result: [JSONNode] = []
        if take(0x5d) { return .array(result) }
        while true {
            skipWhitespace()
            result.append(try parseValue())
            skipWhitespace()
            if take(0x5d) { return .array(result) }
            guard take(0x2c) else { throw WireCodecError.malformedJSON }
        }
    }

    mutating func parseObject() throws -> JSONNode {
        index += 1
        skipWhitespace()
        var result: [(String, JSONNode)] = []
        var keys = Set<String>()
        if take(0x7d) { return .object(result) }
        while true {
            skipWhitespace()
            guard index < bytes.count, bytes[index] == 0x22 else {
                throw WireCodecError.malformedJSON
            }
            let key = try parseString()
            guard keys.insert(key).inserted else { throw WireCodecError.duplicateKey }
            skipWhitespace()
            guard take(0x3a) else { throw WireCodecError.malformedJSON }
            skipWhitespace()
            result.append((key, try parseValue()))
            skipWhitespace()
            if take(0x7d) { return .object(result) }
            guard take(0x2c) else { throw WireCodecError.malformedJSON }
        }
    }

    mutating func parseString() throws -> String {
        guard take(0x22) else { throw WireCodecError.malformedJSON }
        var result = ""
        var chunk: [UInt8] = []
        func decoded(_ value: [UInt8]) throws -> String {
            guard let text = String(bytes: value, encoding: .utf8) else {
                throw WireCodecError.malformedJSON
            }
            return text
        }
        while index < bytes.count {
            let byte = bytes[index]
            index += 1
            if byte == 0x22 {
                result += try decoded(chunk)
                return result
            }
            if byte == 0x5c {
                result += try decoded(chunk)
                chunk.removeAll(keepingCapacity: true)
                guard index < bytes.count else { throw WireCodecError.malformedJSON }
                let escape = bytes[index]
                index += 1
                switch escape {
                case 0x22: result.append("\"")
                case 0x5c: result.append("\\")
                case 0x2f: result.append("/")
                case 0x62: result.append("\u{08}")
                case 0x66: result.append("\u{0c}")
                case 0x6e: result.append("\n")
                case 0x72: result.append("\r")
                case 0x74: result.append("\t")
                case 0x75:
                    let first = try parseHexScalar()
                    if (0xd800...0xdbff).contains(first) {
                        guard take(0x5c), take(0x75) else { throw WireCodecError.malformedJSON }
                        let second = try parseHexScalar()
                        guard (0xdc00...0xdfff).contains(second) else {
                            throw WireCodecError.malformedJSON
                        }
                        let scalar = 0x10000 + ((first - 0xd800) << 10) + second - 0xdc00
                        guard let unicode = UnicodeScalar(scalar) else {
                            throw WireCodecError.malformedJSON
                        }
                        result.unicodeScalars.append(unicode)
                    } else {
                        guard !(0xdc00...0xdfff).contains(first), let unicode = UnicodeScalar(first) else {
                            throw WireCodecError.malformedJSON
                        }
                        result.unicodeScalars.append(unicode)
                    }
                default: throw WireCodecError.malformedJSON
                }
            } else {
                guard byte >= 0x20 else { throw WireCodecError.malformedJSON }
                chunk.append(byte)
            }
        }
        throw WireCodecError.malformedJSON
    }

    mutating func parseHexScalar() throws -> UInt32 {
        guard index + 4 <= bytes.count else { throw WireCodecError.malformedJSON }
        var value: UInt32 = 0
        for byte in bytes[index..<(index + 4)] {
            value <<= 4
            switch byte {
            case 0x30...0x39: value += UInt32(byte - 0x30)
            case 0x41...0x46: value += UInt32(byte - 0x41 + 10)
            case 0x61...0x66: value += UInt32(byte - 0x61 + 10)
            default: throw WireCodecError.malformedJSON
            }
        }
        index += 4
        return value
    }

    mutating func parseNumber() throws -> String {
        let start = index
        _ = take(0x2d)
        guard index < bytes.count else { throw WireCodecError.malformedJSON }
        if take(0x30) {
            if index < bytes.count, (0x30...0x39).contains(bytes[index]) {
                throw WireCodecError.malformedJSON
            }
        } else {
            guard takeDigit(1...9) else { throw WireCodecError.malformedJSON }
            while takeDigit(0...9) {}
        }
        if take(0x2e) {
            guard takeDigit(0...9) else { throw WireCodecError.malformedJSON }
            while takeDigit(0...9) {}
        }
        if index < bytes.count, bytes[index] == 0x65 || (index < bytes.count && bytes[index] == 0x45) {
            index += 1
            if index < bytes.count, bytes[index] == 0x2b || bytes[index] == 0x2d { index += 1 }
            guard takeDigit(0...9) else { throw WireCodecError.malformedJSON }
            while takeDigit(0...9) {}
        }
        let text = String(decoding: bytes[start..<index], as: UTF8.self)
        guard let value = Double(text), value.isFinite else { throw WireCodecError.malformedJSON }
        return text
    }

    mutating func consume(_ literal: StaticString) throws {
        let expected = Array(String(describing: literal).utf8)
        guard bytes[index...].starts(with: expected) else { throw WireCodecError.malformedJSON }
        index += expected.count
    }

    mutating func take(_ byte: UInt8) -> Bool {
        guard index < bytes.count, bytes[index] == byte else { return false }
        index += 1
        return true
    }

    mutating func takeDigit(_ range: ClosedRange<UInt8>) -> Bool {
        guard index < bytes.count,
              (0x30...0x39).contains(bytes[index]),
              range.contains(bytes[index] - 0x30) else { return false }
        index += 1
        return true
    }

    mutating func skipWhitespace() {
        while index < bytes.count, [0x20, 0x09, 0x0a, 0x0d].contains(bytes[index]) { index += 1 }
    }
}

public enum WireCodec {
    public static func canonicalJSON(_ text: String) throws -> String {
        var parser = JSONParser(bytes: Array(text.utf8))
        return emitJSON(try parser.parse())
    }

    public static func requireCanonicalJSON(_ text: String) throws {
        guard try canonicalJSON(text) == text else { throw WireCodecError.noncanonicalJSON }
    }

    public static func canonicalTarget(_ input: String) throws -> String {
        guard !input.contains("#") else { throw WireCodecError.fragmentNotAllowed }
        let pieces = input.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false)
        let rawPath = String(pieces[0])
        guard rawPath.hasPrefix("/") else { throw WireCodecError.invalidTarget }
        let decodedPath = try percentDecode(rawPath)
        var segments: [String] = []
        for segment in decodedPath.split(separator: "/", omittingEmptySubsequences: false) {
            switch segment {
            case "", ".": continue
            case "..":
                guard segments.popLast() != nil else { throw WireCodecError.dotSegmentAboveRoot }
            default: segments.append(percentEncode(String(segment)))
            }
        }
        let path = "/" + segments.joined(separator: "/")
        guard pieces.count == 2, !pieces[1].isEmpty else { return path }
        var pairs: [(String, String)] = []
        for rawPair in pieces[1].split(separator: "&", omittingEmptySubsequences: false) {
            let fields = rawPair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            guard fields.count == 2 else { throw WireCodecError.invalidTarget }
            pairs.append((try percentDecode(String(fields[0])), try percentDecode(String(fields[1]))))
        }
        pairs.sort {
            let left = (percentEncode($0.0), percentEncode($0.1))
            let right = (percentEncode($1.0), percentEncode($1.1))
            return left.0 == right.0 ? left.1 < right.1 : left.0 < right.0
        }
        let query = pairs.map { "\(percentEncode($0.0))=\(percentEncode($0.1))" }.joined(separator: "&")
        return path + "?" + query
    }

    public static func requireCanonicalTarget(_ input: String) throws {
        guard try canonicalTarget(input) == input else { throw WireCodecError.noncanonicalTarget }
    }

    private static func emitJSON(_ value: JSONNode) -> String {
        switch value {
        case .null: return "null"
        case let .bool(value): return value ? "true" : "false"
        case let .number(value): return canonicalNumber(value)
        case let .string(value): return quote(value)
        case let .array(values): return "[" + values.map(emitJSON).joined(separator: ",") + "]"
        case let .object(values):
            let sorted = values.sorted { Array($0.0.utf16).lexicographicallyPrecedes(Array($1.0.utf16)) }
            return "{" + sorted.map { quote($0.0) + ":" + emitJSON($0.1) }.joined(separator: ",") + "}"
        }
    }

    private static func canonicalNumber(_ text: String) -> String {
        let value = Double(text)!
        if value == 0 { return "0" }
        if value.rounded(.towardZero) == value {
            return String(format: "%.0f", locale: Locale(identifier: "en_US_POSIX"), value)
        }
        return String(value)
    }

    private static func quote(_ value: String) -> String {
        var result = "\""
        for scalar in value.unicodeScalars {
            switch scalar.value {
            case 0x22: result += "\\\""
            case 0x5c: result += "\\\\"
            case 0x08: result += "\\b"
            case 0x09: result += "\\t"
            case 0x0a: result += "\\n"
            case 0x0c: result += "\\f"
            case 0x0d: result += "\\r"
            case 0x00...0x1f: result += String(format: "\\u%04x", scalar.value)
            default: result.unicodeScalars.append(scalar)
            }
        }
        return result + "\""
    }

    private static func percentDecode(_ value: String) throws -> String {
        let bytes = Array(value.utf8)
        var decoded: [UInt8] = []
        var index = 0
        while index < bytes.count {
            if bytes[index] == 0x25 {
                guard index + 2 < bytes.count,
                      let byte = UInt8(String(decoding: bytes[(index + 1)...(index + 2)], as: UTF8.self), radix: 16) else {
                    throw WireCodecError.invalidPercentEncoding
                }
                decoded.append(byte)
                index += 3
            } else {
                decoded.append(bytes[index])
                index += 1
            }
        }
        guard let result = String(bytes: decoded, encoding: .utf8) else {
            throw WireCodecError.invalidPercentEncoding
        }
        return result
    }

    private static func percentEncode(_ value: String) -> String {
        value.utf8.map { byte in
            switch byte {
            case 0x41...0x5a, 0x61...0x7a, 0x30...0x39, 0x2d, 0x2e, 0x5f, 0x7e:
                return String(UnicodeScalar(byte))
            default: return String(format: "%%%02X", byte)
            }
        }.joined()
    }
}
