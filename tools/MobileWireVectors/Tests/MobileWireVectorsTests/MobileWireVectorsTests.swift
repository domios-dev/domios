import CryptoKit
import Foundation
import XCTest
@testable import MobileWireVectors

final class MobileWireVectorsTests: XCTestCase {
    private var vectors: [String: Any]!

    override func setUpWithError() throws {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { url.deleteLastPathComponent() }
        url.appendPathComponent("src-tauri/tests/fixtures/telegram269-mobile-core-wire-v1.json")
        vectors = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        )
    }

    func testCanonicalJSONAndTargetVectorsExecute() throws {
        let jcs = dict(vectors, "jcs")
        for vector in dictionaries(jcs, "canonicalization") {
            XCTAssertEqual(try WireCodec.canonicalJSON(string(vector, "input")), string(vector, "expected"))
            XCTAssertNoThrow(try WireCodec.requireCanonicalJSON(string(vector, "expected")))
        }
        for vector in dictionaries(jcs, "refusals") {
            XCTAssertThrowsError(try WireCodec.canonicalJSON(string(vector, "input"))) {
                XCTAssertEqual(($0 as? WireCodecError)?.rawValue, string(vector, "error"))
            }
        }
        for input in strings(jcs, "noncanonical") {
            XCTAssertThrowsError(try WireCodec.requireCanonicalJSON(input)) {
                XCTAssertEqual($0 as? WireCodecError, .noncanonicalJSON)
            }
        }

        let targets = dict(vectors, "target_canonicalization")
        for vector in dictionaries(targets, "accepted") {
            XCTAssertTrue(string(vector, "expected").canBeConverted(to: .ascii))
            XCTAssertEqual(try WireCodec.canonicalTarget(string(vector, "input")), string(vector, "expected"))
            XCTAssertNoThrow(try WireCodec.requireCanonicalTarget(string(vector, "expected")))
        }
        for vector in dictionaries(targets, "refusals") {
            XCTAssertThrowsError(try WireCodec.canonicalTarget(string(vector, "input"))) {
                XCTAssertEqual(($0 as? WireCodecError)?.rawValue, string(vector, "error"))
            }
        }
        for input in strings(targets, "noncanonical") {
            XCTAssertThrowsError(try WireCodec.requireCanonicalTarget(input)) {
                XCTAssertEqual($0 as? WireCodecError, .noncanonicalTarget)
            }
        }
    }

    func testBase64TargetReceiverAndContractBodiesExecute() throws {
        let base64url = dict(vectors, "base64url")
        for vector in dictionaries(base64url, "valid") {
            XCTAssertEqual(try decodeBase64URL(string(vector, "encoded")), unhex(string(vector, "hex")))
        }
        for vector in dictionaries(base64url, "invalid") {
            XCTAssertThrowsError(try decodeBase64URL(string(vector, "encoded"))) {
                XCTAssertEqual(($0 as? VectorError)?.rawValue, string(vector, "error"))
            }
        }

        let target = dict(vectors, "target")
        XCTAssertEqual(hex(targetBytes(target)), string(target, "expected_hex"))
        XCTAssertTrue(receiverAuthorizes(target, instance: "remote-b", tabID: "tab-17", sessionID: "session-23"))
        XCTAssertFalse(receiverAuthorizes(target, instance: "remote-b", tabID: "tab-other", sessionID: "session-23"))
        XCTAssertFalse(receiverAuthorizes(target, instance: "remote-b", tabID: "tab-17", sessionID: "session-other"))

        let contract = dict(vectors, "contract")
        XCTAssertEqual(strings(contract, "grants").count, 5)
        let routes = dict(contract, "routes")
        let exactRoutes: [String: (String, String)] = [
            "pairing_create": ("POST", "/api/mobile/v1/pairing/requests"),
            "pairing_status": ("GET", "/api/mobile/v1/pairing/requests/pair-01"),
            "pairing_approve": ("POST", "/api/mobile/v1/pairing/requests/pair-01/approve"),
            "pairing_reject": ("POST", "/api/mobile/v1/pairing/requests/pair-01/reject"),
            "request_status": ("GET", "/api/mobile/v1/requests/send-01")
        ]
        for (name, expected) in exactRoutes {
            let route = routes[name] as! [String: Any]
            XCTAssertEqual(string(route, "method"), expected.0)
            XCTAssertEqual(string(route, "path"), expected.1)
        }
        for route in routes.values {
            let body = string(route as! [String: Any], "body_jcs")
            XCTAssertEqual(try WireCodec.canonicalJSON(body).utf8.map { $0 }, body.utf8.map { $0 })
        }
        let send = dict(vectors, "send")
        XCTAssertEqual(string(send, "route"), "/api/mobile/v1/gateways/gateway-a/targets/remote-b/tabs/tab-17/sends")
        XCTAssertEqual(try WireCodec.canonicalJSON(string(send, "body_jcs")), string(send, "body_jcs"))

        let ownership = dict(contract, "ownership")
        XCTAssertEqual(statusError(caller: "mc_beta", owner: "mc_alpha", grant: true, active: true), string(ownership, "foreign_status_error"))
        XCTAssertEqual(statusError(caller: "mc_alpha", owner: "mc_alpha", grant: false, active: true), string(ownership, "removed_grant_error"))
        XCTAssertEqual(statusError(caller: "mc_alpha", owner: "mc_alpha", grant: true, active: false), string(ownership, "revoked_client_error"))

        var reservations = ReservationModel()
        XCTAssertEqual(reservations.reserve(client: "mc_alpha", id: "send-01", digest: "body-a"), .reserved)
        XCTAssertEqual(reservations.reserve(client: "mc_alpha", id: "send-01", digest: "body-a"), .reserved)
        XCTAssertEqual(reservations.submissions, 1)
        reservations.recoverAfterLostEvidence()
        XCTAssertEqual(reservations.reserve(client: "mc_alpha", id: "send-01", digest: "body-a"), .unknown)
        XCTAssertEqual(reservations.submissions, 1)
    }

    func testRequestDiscoveryResponseAndSSEBytesExecute() throws {
        let privateKey = try Curve25519.Signing.PrivateKey(
            rawRepresentation: unhex(string(dict(vectors, "keys"), "private_seed_hex"))
        )
        XCTAssertEqual(hex(privateKey.publicKey.rawRepresentation), string(dict(vectors, "keys"), "public_key_hex"))

        let request = dict(vectors, "request")
        let requestWire = try requestBytes(request)
        XCTAssertEqual(hex(requestWire), string(request, "expected_hex"))
        XCTAssertEqual(hex(try privateKey.signature(for: requestWire)), string(request, "expected_signature_hex"))
        let requestSignature = try privateKey.signature(for: requestWire)
        var changedRequest = requestWire
        changedRequest[changedRequest.index(before: changedRequest.endIndex)] ^= 1
        XCTAssertFalse(privateKey.publicKey.isValidSignature(requestSignature, for: changedRequest))
        XCTAssertNoThrow(try WireCodec.requireCanonicalJSON(string(request, "unsigned_proof_jcs")))
        XCTAssertThrowsError(try WireCodec.requireCanonicalJSON(string(request, "noncanonical_proof_jcs")))

        let discovery = dict(vectors, "discovery")
        let discoveryWire = discoveryBytes(discovery)
        XCTAssertEqual(hex(discoveryWire), string(discovery, "expected_hex"))
        XCTAssertEqual(hex(try privateKey.signature(for: discoveryWire)), string(discovery, "expected_signature_hex"))

        let response = dict(vectors, "response")
        let body = Data(string(response, "body_jcs").utf8)
        XCTAssertEqual("sha256:" + hex(Data(SHA256.hash(data: body))), string(response, "response_body_sha256"))
        let responseWire = try responseBytes(response)
        XCTAssertEqual(hex(responseWire), string(response, "expected_hex"))
        XCTAssertEqual(hex(try privateKey.signature(for: responseWire)), string(response, "expected_signature_hex"))
        XCTAssertFalse(string(response, "body_jcs").contains("signature"))
        XCTAssertFalse(string(response, "request_id").isEmpty)
        XCTAssertGreaterThan(uint64(response, "server_time_ms"), 0)
        let headers = dict(response, "transmitted_headers")
        XCTAssertEqual(string(headers, "X-CodeTerm-Mobile-Response-Key"), string(response, "server_key_id"))
        XCTAssertEqual(string(headers, "X-CodeTerm-Mobile-Response-Nonce"), string(response, "response_nonce"))
        XCTAssertEqual(try decodeBase64URL(string(headers, "X-CodeTerm-Mobile-Response-Signature")), try privateKey.signature(for: responseWire))
        XCTAssertEqual(string(headers, "X-CodeTerm-Mobile-Request-Id"), string(response, "request_id"))
        XCTAssertEqual(string(headers, "X-CodeTerm-Mobile-Server-Time-Ms"), String(uint64(response, "server_time_ms")))

        let sse = dict(vectors, "sse")
        let seedInput = sseSeedBytes(sse)
        XCTAssertEqual(hex(seedInput), string(sse, "expected_seed_input_hex"))
        let seed = Data(SHA256.hash(data: seedInput))
        XCTAssertEqual(hex(seed), string(sse, "expected_seed_hex"))
        let frames = sse["frames"] as! [[String: Any]]
        var previous = seed
        for vector in frames {
            XCTAssertEqual(string(vector, "previous_frame_hash"), "sha256:" + hex(previous))
            let frame = sseFrameBytes(vector, previous: previous)
            XCTAssertEqual(hex(frame), string(vector, "expected_frame_hex"))
            let nonce = try decodeBase64URL(string(vector, "response_nonce"))
            let signatureInput = Data("CTMC1-SSE-SIGNATURE\0".utf8) + frame + nonce
            let signature = try privateKey.signature(for: signatureInput)
            XCTAssertEqual(hex(signature), string(vector, "expected_signature_hex"))
            XCTAssertEqual(try decodeBase64URL(string(vector, "signature")), signature)
            XCTAssertEqual(string(vector, "key_id"), string(response, "server_key_id"))
            XCTAssertFalse(string(vector, "signed_payload_jcs").contains("signature"))
            XCTAssertFalse(string(vector, "signed_payload_jcs").contains("previous_frame_hash"))
            let envelope = "{\"key_id\":\"\(string(vector, "key_id"))\",\"previous_frame_hash\":\"\(string(vector, "previous_frame_hash"))\",\"response_nonce\":\"\(string(vector, "response_nonce"))\",\"signature\":\"\(string(vector, "signature"))\",\"signed_payload\":\(string(vector, "signed_payload_jcs"))}"
            XCTAssertEqual(envelope, string(vector, "expected_envelope_jcs"))
            XCTAssertEqual(try WireCodec.canonicalJSON(envelope), envelope)
            previous = Data(SHA256.hash(data: frame))
        }
        var brokenPrevious = seed
        brokenPrevious[brokenPrevious.startIndex] ^= 1
        XCTAssertNotEqual(sseFrameBytes(frames[0], previous: brokenPrevious), sseFrameBytes(frames[0], previous: seed))
    }

    private func requestBytes(_ vector: [String: Any]) throws -> Data {
        var out = Data("CTMC1-REQUEST\0".utf8)
        for key in ["proof_type", "client_id", "key_fingerprint", "server_instance_id", "endpoint_id", "method", "canonical_target"] {
            appendField(&out, string(vector, key))
        }
        out += digest(vector, "body_sha256")
        appendUInt64(&out, uint64(vector, "issued_at_ms"))
        appendUInt64(&out, uint64(vector, "expires_at_ms"))
        out += try decodeBase64URL(string(vector, "nonce_128"))
        return out
    }

    private func discoveryBytes(_ vector: [String: Any]) -> Data {
        var out = Data("CTMC1-DISCOVERY\0".utf8)
        for key in ["endpoint_id", "origin", "transport", "server_instance_id", "server_key_id", "server_key_fingerprint", "tls_spki_sha256", "protocol_version"] {
            appendField(&out, string(vector, key))
        }
        appendUInt64(&out, uint64(vector, "directory_revision"))
        appendField(&out, string(vector, "build_sha"))
        out += unhex(string(vector, "server_public_key_hex"))
        return out
    }

    private func responseBytes(_ vector: [String: Any]) throws -> Data {
        var out = Data("CTMC1-RESPONSE\0".utf8)
        for key in ["status", "request_method", "canonical_target"] { appendField(&out, string(vector, key)) }
        out += try decodeBase64URL(string(vector, "request_nonce"))
        for key in ["request_id", "client_id", "server_instance_id", "server_key_id"] { appendField(&out, string(vector, key)) }
        out += digest(vector, "request_body_sha256")
        out += digest(vector, "response_body_sha256")
        out += try decodeBase64URL(string(vector, "response_nonce"))
        appendUInt64(&out, uint64(vector, "server_time_ms"))
        return out
    }

    private func sseSeedBytes(_ vector: [String: Any]) -> Data {
        var out = Data("CTMC1-SSE-SEED\0".utf8)
        for key in ["subscription_id", "boot_id", "client_id", "server_instance_id", "endpoint_id"] { appendField(&out, string(vector, key)) }
        out += digest(vector, "target_set_sha256")
        appendUInt64(&out, uint64(vector, "grant_revision"))
        return out
    }

    private func sseFrameBytes(_ vector: [String: Any], previous: Data) -> Data {
        var out = Data("CTMC1-SSE-FRAME\0".utf8)
        appendUInt64(&out, uint64(vector, "sequence"))
        out += previous
        for key in ["subscription_id", "client_id", "server_instance_id", "endpoint_id"] { appendField(&out, string(vector, key)) }
        appendField(&out, string(vector, "signed_payload_jcs"))
        return out
    }

    private func targetBytes(_ target: [String: Any]) -> Data {
        var out = Data("CTMC1-TARGET\0".utf8)
        guard string(target, "kind") == "tab" else { return out + Data([0]) }
        out.append(1)
        appendField(&out, string(target, "gateway_instance_id"))
        if let peer = target["peer_id"] as? String {
            out.append(1)
            appendField(&out, peer)
        } else {
            out.append(0)
        }
        for key in ["target_instance_id", "tab_id", "session_id"] { appendField(&out, string(target, key)) }
        appendUInt64(&out, uint64(target, "lifecycle_epoch"))
        out += unhex(string(target, "binding_nonce_digest_hex"))
        return out
    }

    private func receiverAuthorizes(_ target: [String: Any], instance: String, tabID: String, sessionID: String) -> Bool {
        string(target, "target_instance_id") == instance
            && string(target, "tab_id") == tabID
            && string(target, "session_id") == sessionID
            && uint64(target, "lifecycle_epoch") == 42
            && string(target, "binding_nonce_digest_hex") == hex(Data(SHA256.hash(data: Data("binding-live-29".utf8))))
    }

    private func statusError(caller: String, owner: String, grant: Bool, active: Bool) -> String? {
        if !active { return "client_revoked" }
        if caller != owner { return "request_not_found" }
        if !grant { return "grant_denied" }
        return nil
    }

    private func appendField(_ data: inout Data, _ text: String) {
        let bytes = Data(text.utf8)
        appendUInt32(&data, UInt32(bytes.count))
        data += bytes
    }

    private func appendUInt32(_ data: inout Data, _ value: UInt32) {
        data.append(contentsOf: withUnsafeBytes(of: value.bigEndian, Array.init))
    }

    private func appendUInt64(_ data: inout Data, _ value: UInt64) {
        data.append(contentsOf: withUnsafeBytes(of: value.bigEndian, Array.init))
    }

    private func digest(_ object: [String: Any], _ key: String) -> Data {
        unhex(String(string(object, key).dropFirst("sha256:".count)))
    }

    private func decodeBase64URL(_ value: String) throws -> Data {
        guard !value.isEmpty else { throw VectorError.empty }
        let alphabet = Array("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_".utf8)
        guard value.utf8.allSatisfy({ alphabet.contains($0) }) else { throw VectorError.nonalphabet }
        if value.count % 4 == 1 { throw VectorError.remainderOne }
        let last = alphabet.firstIndex(of: value.utf8.last!)!
        if value.count % 4 == 2, last & 0x0f != 0 { throw VectorError.nonzeroUnusedBits }
        if value.count % 4 == 3, last & 0x03 != 0 { throw VectorError.nonzeroUnusedBits }
        let base64 = value.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
            + String(repeating: "=", count: (4 - value.count % 4) % 4)
        guard let data = Data(base64Encoded: base64) else { throw VectorError.invalidEncoding }
        return data
    }

    private func unhex(_ value: String) -> Data {
        Data(stride(from: 0, to: value.count, by: 2).map { offset in
            let start = value.index(value.startIndex, offsetBy: offset)
            return UInt8(value[start..<value.index(start, offsetBy: 2)], radix: 16)!
        })
    }

    private func hex(_ data: Data) -> String { data.map { String(format: "%02x", $0) }.joined() }
    private func dict(_ object: [String: Any], _ key: String) -> [String: Any] { object[key] as! [String: Any] }
    private func string(_ object: [String: Any], _ key: String) -> String { object[key] as! String }
    private func strings(_ object: [String: Any], _ key: String) -> [String] { object[key] as! [String] }
    private func dictionaries(_ object: [String: Any], _ key: String) -> [[String: Any]] { object[key] as! [[String: Any]] }
    private func uint64(_ object: [String: Any], _ key: String) -> UInt64 { (object[key] as! NSNumber).uint64Value }
}

private enum VectorError: String, Error {
    case empty
    case remainderOne = "remainder_one"
    case nonalphabet
    case nonzeroUnusedBits = "nonzero_unused_bits"
    case invalidEncoding = "invalid_encoding"
}

private enum ReservationState: Equatable {
    case reserved
    case unknown
}

private struct ReservationModel {
    private var entries: [String: (digest: String, state: ReservationState)] = [:]
    private(set) var submissions = 0

    mutating func reserve(client: String, id: String, digest: String) -> ReservationState? {
        let key = client + "\0" + id
        if let entry = entries[key] {
            return entry.digest == digest ? entry.state : nil
        }
        entries[key] = (digest, .reserved)
        submissions += 1
        return .reserved
    }

    mutating func recoverAfterLostEvidence() {
        for key in Array(entries.keys) {
            entries[key] = (entries[key]!.digest, .unknown)
        }
    }
}
