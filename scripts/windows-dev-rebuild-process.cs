using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

public interface IDevRebuildProcess
{
    bool WaitForExit(int milliseconds);
    void Kill();
}

public sealed class DevRebuildProcess : IDevRebuildProcess, IDisposable
{
    const uint PROCESS_TERMINATE = 0x0001;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const uint SYNCHRONIZE = 0x00100000;
    const uint WAIT_OBJECT_0 = 0;
    const uint WAIT_TIMEOUT = 258;
    readonly SafeProcessHandle handle;
    public int Id { get; private set; }

    [StructLayout(LayoutKind.Sequential)]
    struct FileInformation
    {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Created, Accessed, Written;
        public uint VolumeSerial, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetFileInformationByHandle(SafeFileHandle file, out FileInformation information);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern uint GetFinalPathNameByHandle(SafeFileHandle file, StringBuilder path, uint length, uint flags);

    public static string PhysicalImage(string path)
    {
        using (var file = CreateFile(path, 0, 7, IntPtr.Zero, 3, 0, IntPtr.Zero))
        {
            if (file.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error(), "Open DEV image");
            var result = new StringBuilder(32768);
            uint count = GetFinalPathNameByHandle(file, result, (uint)result.Capacity, 0);
            if (count == 0 || count >= result.Capacity)
                throw new Win32Exception(Marshal.GetLastWin32Error(), "Resolve DEV image");
            string physical = result.ToString();
            return physical.StartsWith(@"\\?\") ? physical.Substring(4) : physical;
        }
    }

    public static string FileIdentity(string path)
    {
        using (var file = CreateFile(path, 0, 7, IntPtr.Zero, 3, 0, IntPtr.Zero))
        {
            if (file.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error(), "Open DEV image");
            FileInformation information;
            if (!GetFileInformationByHandle(file, out information))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "GetFileInformationByHandle(DEV image)");
            return information.VolumeSerial.ToString("X8") + ":" + information.IndexHigh.ToString("X8") + information.IndexLow.ToString("X8");
        }
    }

    public string ImageIdentity { get { return FileIdentity(Executable); } }

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern SafeProcessHandle OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetProcessTimes(SafeProcessHandle process, out long created, out long exited, out long kernel, out long user);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool QueryFullProcessImageName(SafeProcessHandle process, uint flags, StringBuilder name, ref int length);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool TerminateProcess(SafeProcessHandle process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(SafeProcessHandle process, uint milliseconds);

    DevRebuildProcess(SafeProcessHandle process, int pid) { handle = process; Id = pid; }

    public static DevRebuildProcess Open(int pid)
    {
        var process = OpenProcess(SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (!process.IsInvalid) return new DevRebuildProcess(process, pid);
        int error = Marshal.GetLastWin32Error();
        process.Dispose();
        if (error == 87) return null;
        throw new Win32Exception(error, "OpenProcess(DEV)");
    }

    public string Created
    {
        get
        {
            long created, exited, kernel, user;
            if (!GetProcessTimes(handle, out created, out exited, out kernel, out user))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "GetProcessTimes(DEV)");
            return created.ToString(System.Globalization.CultureInfo.InvariantCulture);
        }
    }

    public string Executable
    {
        get
        {
            int length = 32768;
            var name = new StringBuilder(length);
            if (!QueryFullProcessImageName(handle, 0, name, ref length))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "QueryFullProcessImageName(DEV)");
            return name.ToString();
        }
    }

    public bool WaitForExit(int milliseconds)
    {
        if (milliseconds < 0) throw new ArgumentOutOfRangeException("milliseconds");
        uint result = WaitForSingleObject(handle, (uint)milliseconds);
        if (result == WAIT_OBJECT_0) return true;
        if (result == WAIT_TIMEOUT) return false;
        if (result == 0xFFFFFFFF) throw new Win32Exception(Marshal.GetLastWin32Error(), "WaitForSingleObject(DEV)");
        throw new InvalidOperationException("Unexpected DEV process wait result: " + result);
    }

    public void Kill()
    {
        if (!TerminateProcess(handle, 1))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "TerminateProcess(DEV)");
    }

    public static void Stop(IDevRebuildProcess process, int milliseconds)
    {
        if (process.WaitForExit(0)) return;
        try { process.Kill(); }
        catch (Win32Exception error) { if (error.NativeErrorCode != 5) throw; }
        if (!process.WaitForExit(milliseconds))
            throw new InvalidOperationException("DEV pinned process did not exit; refusing staging");
    }

    public void Dispose() { handle.Dispose(); }
}
