// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "BranchBoxApp",
    defaultLocalization: "en",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(
            name: "BranchBoxApp",
            targets: ["BranchBoxApp"]
        )
    ],
    dependencies: [
        .package(url: "https://github.com/grpc/grpc-swift.git", from: "1.27.0"),
        .package(url: "https://github.com/apple/swift-protobuf.git", from: "1.26.0"),
        // Explicitly depend on SwiftNIO to use NIOCore/NIOPosix products
        .package(url: "https://github.com/apple/swift-nio.git", from: "2.88.0")
    ],
    targets: [
        .executableTarget(
            name: "BranchBoxApp",
            dependencies: [
                .product(name: "GRPC", package: "grpc-swift"),
                .product(name: "NIOCore", package: "swift-nio"),
                .product(name: "NIOPosix", package: "swift-nio"),
                .product(name: "SwiftProtobuf", package: "swift-protobuf")
            ],
            path: "Sources"
        ),
        .testTarget(
            name: "BranchBoxAppTests",
            dependencies: ["BranchBoxApp"],
            path: "Tests"
        )
    ]
)
