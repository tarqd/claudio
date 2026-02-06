// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "ClaudioUI",
    platforms: [.macOS(.v26)],
    targets: [
        .executableTarget(
            name: "ClaudioUI",
            path: "Sources/ClaudioUI",
            linkerSettings: [
                .unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks"]),
            ]
        ),
    ]
)
