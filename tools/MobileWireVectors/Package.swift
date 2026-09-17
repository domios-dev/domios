// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "MobileWireVectors",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "MobileWireVectors", targets: ["MobileWireVectors"])
    ],
    targets: [
        .target(name: "MobileWireVectors"),
        .testTarget(
            name: "MobileWireVectorsTests",
            dependencies: ["MobileWireVectors"]
        )
    ]
)
