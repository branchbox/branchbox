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
        .package(url: "https://github.com/apple/swift-protobuf.git", from: "1.26.0")
    ],
    targets: [
        .executableTarget(
            name: "BranchBoxApp",
            dependencies: [
                .product(name: "GRPC", package: "grpc-swift"),
                .product(name: "NIOCore", package: "grpc-swift"),
                .product(name: "NIOPosix", package: "grpc-swift"),
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
