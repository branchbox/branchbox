// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "BranchBox",
    defaultLocalization: "en",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "BranchBox", targets: ["BranchBoxApp"])],
    targets: [
        .target(name: "BranchBoxKit"),
        .target(name: "BranchBoxCLI", dependencies: ["BranchBoxKit"]),
        .target(name: "BranchBoxStores", dependencies: ["BranchBoxKit"]),
        .target(name: "BranchBoxPreview", dependencies: ["BranchBoxKit"]),
        .executableTarget(name: "BranchBoxApp",
                          dependencies: ["BranchBoxKit", "BranchBoxCLI", "BranchBoxStores", "BranchBoxPreview"]),
        .target(name: "BranchBoxTestSupport",
                dependencies: ["BranchBoxKit", "BranchBoxCLI", "BranchBoxPreview"],
                path: "Tests/BranchBoxTestSupport",
                resources: [.copy("Fixtures")]),
        .testTarget(name: "BranchBoxKitTests", dependencies: ["BranchBoxKit", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxCLITests", dependencies: ["BranchBoxCLI", "BranchBoxKit", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxStoresTests", dependencies: ["BranchBoxStores", "BranchBoxPreview", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxAppTests", dependencies: ["BranchBoxApp", "BranchBoxStores", "BranchBoxPreview", "BranchBoxTestSupport"]),
        .testTarget(name: "BranchBoxIntegrationTests",
                    dependencies: ["BranchBoxCLI", "BranchBoxKit", "BranchBoxStores", "BranchBoxTestSupport"]),
    ],
    swiftLanguageModes: [.v6]
)
