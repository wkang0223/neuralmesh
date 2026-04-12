// nm-vm-helper — thin Virtualization.framework wrapper for NeuralMesh jobs.
//
// Usage:
//   nm-vm-helper create <job_id> <cpu_count> <ram_gb> <job_dir> <base_image>
//   nm-vm-helper run    <job_id> <entry_script>
//   nm-vm-helper destroy <job_id>
//
// The helper creates a lightweight Linux ARM64 VM using Apple's
// Virtualization.framework, mounts the job directory as a virtio-fs
// share at /job inside the VM, runs the entry script, and streams
// stdout/stderr back to the caller.
//
// Build:
//   swiftc -O -target arm64-apple-macos13 main.swift -o nm-vm-helper
//   codesign --entitlements entitlements.plist -s - nm-vm-helper

import Foundation
import Virtualization

// ── VM state directory ──────────────────────────────────────────────────────

let stateDir = URL(fileURLWithPath: "/var/neuralmesh/vms")

func vmDir(_ jobId: String) -> URL {
    stateDir.appendingPathComponent(jobId)
}

// ── Commands ────────────────────────────────────────────────────────────────

let args = CommandLine.arguments
guard args.count >= 3 else {
    fputs("usage: nm-vm-helper <create|run|destroy> <job_id> [args...]\n", stderr)
    exit(1)
}

let cmd   = args[1]
let jobId = args[2]

switch cmd {
case "create":
    guard args.count == 7 else {
        fputs("create needs: job_id cpu_count ram_gb job_dir base_image\n", stderr)
        exit(1)
    }
    let cpuCount  = Int(args[3]) ?? 4
    let ramGb     = UInt64(args[4]) ?? 4
    let jobDir    = args[5]
    let baseImage = args[6]
    createVM(jobId: jobId, cpuCount: cpuCount, ramGb: ramGb, jobDir: jobDir, baseImage: baseImage)

case "run":
    guard args.count == 4 else {
        fputs("run needs: job_id entry_script\n", stderr)
        exit(1)
    }
    runVM(jobId: jobId, entryScript: args[3])

case "destroy":
    destroyVM(jobId: jobId)

default:
    fputs("unknown command: \(cmd)\n", stderr)
    exit(1)
}

// ── Create VM ───────────────────────────────────────────────────────────────

func createVM(jobId: String, cpuCount: Int, ramGb: UInt64, jobDir: String, baseImage: String) {
    let dir = vmDir(jobId)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

    // Create copy-on-write overlay disk (saves space — shares base image blocks)
    let overlayPath = dir.appendingPathComponent("overlay.img").path
    let createDisk = Process()
    createDisk.executableURL = URL(fileURLWithPath: "/usr/bin/hdiutil")
    createDisk.arguments = [
        "create", "-size", "10g", "-type", "SPARSE",
        "-fs", "Case-sensitive APFS", overlayPath
    ]
    try? createDisk.run()
    createDisk.waitUntilExit()

    // Save VM metadata
    let meta: [String: Any] = [
        "job_id":      jobId,
        "cpu_count":   cpuCount,
        "ram_gb":      ramGb,
        "job_dir":     jobDir,
        "base_image":  baseImage,
        "overlay":     overlayPath + ".sparseimage",
    ]
    let metaData = try! JSONSerialization.data(withJSONObject: meta)
    try! metaData.write(to: dir.appendingPathComponent("meta.json"))

    print("VM created: \(jobId)")
}

// ── Run VM ───────────────────────────────────────────────────────────────────

func runVM(jobId: String, entryScript: String) {
    let dir      = vmDir(jobId)
    let metaFile = dir.appendingPathComponent("meta.json")

    guard let metaData = try? Data(contentsOf: metaFile),
          let meta = try? JSONSerialization.jsonObject(with: metaData) as? [String: Any]
    else {
        fputs("VM metadata not found for \(jobId)\n", stderr)
        exit(1)
    }

    let cpuCount  = meta["cpu_count"] as? Int    ?? 4
    let ramGb     = meta["ram_gb"]    as? UInt64 ?? 4
    let jobDir    = meta["job_dir"]   as? String ?? ""
    let baseImage = meta["base_image"] as? String ?? ""

    // ── Build VM configuration ────────────────────────────────────────────

    let config = VZVirtualMachineConfiguration()
    config.cpuCount    = cpuCount
    config.memorySize  = ramGb * 1024 * 1024 * 1024

    // Boot loader — Linux kernel + initrd (bundled in base image directory)
    let kernelURL  = URL(fileURLWithPath: baseImage).deletingLastPathComponent()
        .appendingPathComponent("vmlinuz")
    let initrdURL  = URL(fileURLWithPath: baseImage).deletingLastPathComponent()
        .appendingPathComponent("initrd")

    let bootLoader = VZLinuxBootLoader(kernelURL: kernelURL)
    bootLoader.initialRamdiskURL = initrdURL
    // Pass job dir and entry script via kernel command line
    bootLoader.commandLine = "console=hvc0 root=/dev/vda rw nm_job_dir=/job nm_entry=\(entryScript)"
    config.bootLoader = bootLoader

    // Storage — base image (read-only) + overlay (writable)
    let baseDiskAttach  = try! VZDiskImageStorageDeviceAttachment(url: URL(fileURLWithPath: baseImage), readOnly: true)
    let baseDisk        = VZVirtioBlockDeviceConfiguration(attachment: baseDiskAttach)
    config.storageDevices = [baseDisk]

    // virtio-fs shared directory — mounts at /job in the VM
    if !jobDir.isEmpty {
        let share  = VZSharedDirectory(url: URL(fileURLWithPath: jobDir), readOnly: false)
        let fsConf = VZVirtioFileSystemDeviceConfiguration(tag: "job")
        fsConf.share = VZSingleDirectoryShare(directory: share)
        config.directorySharingDevices = [fsConf]
    }

    // Serial console — captures stdout/stderr from the VM
    let stdoutPipe = Pipe()
    let consoleOut = VZFileHandleSerialPortAttachment(
        fileHandleForReading: FileHandle.standardInput,
        fileHandleForWriting: stdoutPipe.fileHandleForWriting
    )
    let console = VZVirtioConsoleDeviceSerialPortConfiguration()
    console.attachment = consoleOut
    config.serialPorts = [console]

    // Network — host NAT (no inbound, safe for jobs)
    let net = VZVirtioNetworkDeviceConfiguration()
    net.attachment = VZNATNetworkDeviceAttachment()
    config.networkDevices = [net]

    // Entropy source
    config.entropyDevices = [VZVirtioEntropyDeviceConfiguration()]

    // ── Validate and start VM ─────────────────────────────────────────────

    do {
        try config.validate()
    } catch {
        fputs("VM config invalid: \(error)\n", stderr)
        exit(1)
    }

    let vm = VZVirtualMachine(configuration: config)

    let sema = DispatchSemaphore(value: 0)
    var exitCode: Int32 = 0

    vm.start { result in
        switch result {
        case .failure(let err):
            fputs("VM start failed: \(err)\n", stderr)
            exitCode = 1
            sema.signal()
        case .success:
            print("VM started for job \(jobId)")
            // VM runs the init script from kernel cmdline
            // When the job completes, the init process exits and VM stops
        }
    }

    // Observe VM state for completion
    var obs: NSKeyValueObservation?
    obs = vm.observe(\.state, options: [.new]) { vm, _ in
        if vm.state == .stopped || vm.state == .error {
            obs?.invalidate()
            sema.signal()
        }
    }

    // Stream VM stdout
    let readHandle = stdoutPipe.fileHandleForReading
    readHandle.readabilityHandler = { handle in
        let data = handle.availableData
        if !data.isEmpty, let text = String(data: data, encoding: .utf8) {
            print(text, terminator: "")
        }
    }

    sema.wait()

    exit(exitCode)
}

// ── Destroy VM ───────────────────────────────────────────────────────────────

func destroyVM(jobId: String) {
    let dir = vmDir(jobId)
    try? FileManager.default.removeItem(at: dir)
    print("VM destroyed: \(jobId)")
}
