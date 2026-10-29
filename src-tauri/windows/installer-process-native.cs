using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;

public sealed class DomiosInstallHolder {
    public uint Pid;
    public ulong Created;
}

public static class DomiosInstallerProcess {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder image, ref uint size);

    [StructLayout(LayoutKind.Sequential)]
    struct UniqueProcess {
        public uint Pid;
        public FILETIME Created;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct ProcessInfo {
        public UniqueProcess Process;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string Name;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string Service;
        public uint Type;
        public uint Status;
        public uint Session;
        [MarshalAs(UnmanagedType.Bool)] public bool Restartable;
    }
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    static extern uint RmStartSession(out uint session, uint flags, StringBuilder key);
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    static extern uint RmRegisterResources(uint session, uint fileCount, string[] files, uint appCount, IntPtr apps, uint serviceCount, IntPtr services);
    [DllImport("rstrtmgr.dll")]
    static extern uint RmGetList(uint session, out uint needed, ref uint count, [In, Out] ProcessInfo[] processes, ref uint reasons);
    [DllImport("rstrtmgr.dll")]
    static extern uint RmEndSession(uint session);

    static void RequireSuccess(uint result) {
        if (result != 0) throw new InvalidOperationException("Install-holder discovery failed (" + result + ").");
    }

    public static DomiosInstallHolder[] Holders(string[] files) {
        if (files.Length == 0) return new DomiosInstallHolder[0];
        uint session;
        RequireSuccess(RmStartSession(out session, 0, new StringBuilder(33)));
        try {
            RequireSuccess(RmRegisterResources(session, (uint)files.Length, files, 0, IntPtr.Zero, 0, IntPtr.Zero));
            uint needed, count = 0, reasons = 0;
            uint result = RmGetList(session, out needed, ref count, null, ref reasons);
            if (result == 0) return new DomiosInstallHolder[0];
            if (result != 234) RequireSuccess(result);
            var processes = new ProcessInfo[needed];
            count = needed;
            RequireSuccess(RmGetList(session, out needed, ref count, processes, ref reasons));
            var holders = new DomiosInstallHolder[count];
            for (uint index = 0; index < count; index++) {
                holders[index] = new DomiosInstallHolder {
                    Pid = processes[index].Process.Pid,
                    Created = ((ulong)(uint)processes[index].Process.Created.dwHighDateTime << 32) |
                        (uint)processes[index].Process.Created.dwLowDateTime
                };
            }
            return holders;
        } finally {
            RmEndSession(session);
        }
    }
}
