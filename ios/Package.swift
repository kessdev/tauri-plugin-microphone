// swift-tools-version:5.3
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
  name: "tauri-plugin-microphone",
  platforms: [
    .iOS(.v13),
  ],
  products: [
    .library(
      name: "tauri-plugin-microphone",
      type: .static,
      targets: ["tauri-plugin-microphone"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-microphone",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources"),
    .testTarget(
      name: "tauri-plugin-microphoneTests",
      dependencies: [
        "tauri-plugin-microphone"
      ],
      path: "Tests")
  ]
)
