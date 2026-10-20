# Runs $env:BREAKAWAY_COMMAND_LINE outside the calling tab's kill-on-close job and waits for it.
# Exit code 250 means the job refused breakaway; the caller then runs the command in place.
param([Parameter(Mandatory = $true)][string]$Log)

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
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);

    const uint BreakawayFromJob = 0x01000000, NewProcessGroup = 0x00000200, NoWindow = 0x08000000;
    const int UseStdHandles = 0x100, AccessDenied = 5;

    public static int Run(string commandLine, string log) {
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
            WaitForSingleObject(info.process, 0xFFFFFFFF);
            uint code;
            GetExitCodeProcess(info.process, out code);
            CloseHandle(info.thread);
            CloseHandle(info.process);
            return (int)code;
        }
    }
}
'@

exit [DomiosBreakaway]::Run($env:BREAKAWAY_COMMAND_LINE, $Log)
