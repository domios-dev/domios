# Runs outside the tab job; ProcessRecord returns after recording the detached child's identity.
# Exit code 250 means breakaway was refused; background launches must not fall back to containment.
param(
  [Parameter(Mandatory = $true)][string]$Log,
  [string]$ProcessRecord
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class DomiosBreakaway {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct StartupInfo {
        public int cb; public string reserved, desktop, title;
        public int x, y, xSize, ySize, xChars, yChars, fillAttribute, flags;
        public short showWindow, reserved2; public IntPtr reserved3, stdInput, stdOutput, stdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct ProcessInformation { public IntPtr process, thread; public int processId, threadId; }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool CreateProcessW(string application, StringBuilder commandLine, IntPtr processAttributes,
        IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string cwd,
        ref StartupInfo startup, out ProcessInformation info);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr handle, out uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr handle, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetProcessTimes(IntPtr handle, out long created, out long exited, out long kernel, out long user);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool QueryFullProcessImageName(IntPtr handle, uint flags, StringBuilder name, ref int length);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);

    const uint BreakawayFromJob = 0x01000000, NewProcessGroup = 0x00000200, NoWindow = 0x08000000;
    const int UseStdHandles = 0x100, AccessDenied = 5;

    public static int Run(string commandLine, string log, Action<int, string, string> record) {
        using (var file = new FileStream(log, FileMode.Append, FileAccess.Write, FileShare.ReadWrite)) {
            var handle = file.SafeFileHandle.DangerousGetHandle();
            SetHandleInformation(handle, 1, 1);
            var startup = new StartupInfo();
            startup.cb = Marshal.SizeOf(startup);
            startup.flags = UseStdHandles;
            startup.stdOutput = handle;
            startup.stdError = handle;
            ProcessInformation info;
            if (!CreateProcessW(null, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, true,
                    BreakawayFromJob | NewProcessGroup | NoWindow, IntPtr.Zero, Environment.CurrentDirectory,
                    ref startup, out info)) {
                var error = Marshal.GetLastWin32Error();
                return error == AccessDenied ? 250 : 251;
            }
            try {
                if (record != null) {
                    long created, exited, kernel, user;
                    int length = 32768;
                    var image = new StringBuilder(length);
                    if (!GetProcessTimes(info.process, out created, out exited, out kernel, out user) ||
                        !QueryFullProcessImageName(info.process, 0, image, ref length))
                        throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Record detached DEV process identity");
                    record(info.processId, created.ToString(System.Globalization.CultureInfo.InvariantCulture), image.ToString());
                    return 0;
                }
                WaitForSingleObject(info.process, 0xFFFFFFFF);
                uint code;
                GetExitCodeProcess(info.process, out code);
                return (int)code;
            } catch {
                TerminateProcess(info.process, 1);
                throw;
            } finally {
                CloseHandle(info.thread);
                CloseHandle(info.process);
            }
        }
    }
}
'@

$record = $null
if ($ProcessRecord) {
  $record = [Action[int, string, string]] {
    param($processId, $created, $executable)
    @{ pid = $processId; created = $created; executable = $executable } |
      ConvertTo-Json -Compress | Set-Content -LiteralPath $ProcessRecord -Encoding UTF8
  }
}
exit [DomiosBreakaway]::Run($env:BREAKAWAY_COMMAND_LINE, $Log, $record)
