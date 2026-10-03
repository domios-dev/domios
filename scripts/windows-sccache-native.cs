using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Net;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;
public static class WindowsSccacheNative {
    const uint CREATE_SUSPENDED = 0x00000004;
    const uint CREATE_NEW_PROCESS_GROUP = 0x00000200;
    const uint CREATE_NO_WINDOW = 0x08000000;
    const uint CREATE_UNICODE_ENVIRONMENT = 0x00000400;
    const uint EXTENDED_STARTUPINFO_PRESENT = 0x00080000;
    const uint STARTF_USESTDHANDLES = 0x00000100;
    const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
    const int JobObjectBasicAccountingInformation = 1;
    const int JobObjectBasicProcessIdList = 3;
    const int JobObjectAssociateCompletionPortInformation = 7;
    const int JobObjectExtendedLimitInformation = 9;
    const uint JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO = 4;
    const int PROC_THREAD_ATTRIBUTE_HANDLE_LIST = 0x00020002;
    const uint STILL_ACTIVE = 259;
    const uint WAIT_OBJECT_0 = 0;
    const uint WAIT_TIMEOUT = 258;
    const uint WAIT_FAILED = UInt32.MaxValue;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const uint SYNCHRONIZE = 0x00100000;
    const uint TOKEN_QUERY = 0x0008;
    const uint GENERIC_READ = 0x80000000;
    const uint FILE_SHARE_READ = 1;
    const uint FILE_SHARE_WRITE = 2;
    const uint OPEN_EXISTING = 3;
    const uint FILE_ATTRIBUTE_NORMAL = 0x80;
    const uint HANDLE_FLAG_INHERIT = 1;
    [StructLayout(LayoutKind.Sequential)] struct SECURITY_ATTRIBUTES { public int nLength; public IntPtr lpSecurityDescriptor; public int bInheritHandle; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct STARTUPINFO { public int cb; public string lpReserved; public string lpDesktop; public string lpTitle; public uint dwX; public uint dwY; public uint dwXSize; public uint dwYSize; public uint dwXCountChars; public uint dwYCountChars; public uint dwFillAttribute; public uint dwFlags; public ushort wShowWindow; public ushort cbReserved2; public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError; }
    [StructLayout(LayoutKind.Sequential)] struct STARTUPINFOEX { public STARTUPINFO StartupInfo; public IntPtr lpAttributeList; }
    [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr hProcess; public IntPtr hThread; public uint dwProcessId; public uint dwThreadId; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_BASIC_LIMIT_INFORMATION { public long PerProcessUserTimeLimit; public long PerJobUserTimeLimit; public uint LimitFlags; public UIntPtr MinimumWorkingSetSize; public UIntPtr MaximumWorkingSetSize; public uint ActiveProcessLimit; public UIntPtr Affinity; public uint PriorityClass; public uint SchedulingClass; }
    [StructLayout(LayoutKind.Sequential)] struct IO_COUNTERS { public ulong ReadOperationCount; public ulong WriteOperationCount; public ulong OtherOperationCount; public ulong ReadTransferCount; public ulong WriteTransferCount; public ulong OtherTransferCount; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION { public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation; public IO_COUNTERS IoInfo; public UIntPtr ProcessMemoryLimit; public UIntPtr JobMemoryLimit; public UIntPtr PeakProcessMemoryUsed; public UIntPtr PeakJobMemoryUsed; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_ASSOCIATE_COMPLETION_PORT { public IntPtr CompletionKey; public IntPtr CompletionPort; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION { public long TotalUserTime; public long TotalKernelTime; public long ThisPeriodTotalUserTime; public long ThisPeriodTotalKernelTime; public uint TotalPageFaults; public uint TotalProcesses; public uint ActiveProcesses; public uint TotalTerminatedProcesses; }
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool CreatePipe(out IntPtr hReadPipe, out IntPtr hWritePipe, ref SECURITY_ATTRIBUTES attrs, uint size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern IntPtr CreateFileW(string name, uint access, uint share, ref SECURITY_ATTRIBUTES attrs, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern bool CreateProcessW(string app, StringBuilder command, IntPtr procAttrs, IntPtr threadAttrs, bool inherit, uint flags, StringBuilder environment, string cwd, ref STARTUPINFOEX startup, out PROCESS_INFORMATION info);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern bool CreateDirectoryW(string path, IntPtr securityAttributes);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetProcessTimes(IntPtr process, out long creation, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern bool QueryFullProcessImageNameW(IntPtr process, uint flags, StringBuilder image, ref int size);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool GetTokenInformation(IntPtr token, int infoClass, IntPtr info, int length, out int needed);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern IntPtr CreateJobObjectW(IntPtr attrs, string name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length, out uint needed);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateIoCompletionPort(IntPtr file, IntPtr existing, UIntPtr key, uint threads);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetQueuedCompletionStatus(IntPtr port, out uint bytes, out UIntPtr key, out IntPtr overlapped, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("iphlpapi.dll", SetLastError = true)] static extern int GetExtendedTcpTable(byte[] table, ref int size, bool order, int family, int tableClass, uint reserved);
    static Exception Last(string op) { return new Win32Exception(Marshal.GetLastWin32Error(), op); }
    static void Need(bool ok, string op) { if (!ok) throw Last(op); }
    static bool WaitSignaledOrTimeout(IntPtr handle, int ms, string op) { uint result = WaitForSingleObject(handle, (uint)Math.Max(0, ms)); if (result == WAIT_OBJECT_0) return true; if (result == WAIT_TIMEOUT) return false; if (result == WAIT_FAILED) throw Last(op); throw new InvalidOperationException(op + "_unexpected_wait_result_" + result); }
    static int Remaining(long until) { long left = until - Environment.TickCount64; return left <= 0 ? 0 : (int)Math.Min(Int32.MaxValue, left); }
    public sealed class BoundedTail {
        readonly object gate = new object();
        readonly byte[] tail;
        int used;
        long total;
        bool truncated;
        public BoundedTail(int capacity) { if (capacity < 1) throw new ArgumentOutOfRangeException("capacity"); tail = new byte[capacity]; }
        public void Append(byte[] source, int count) {
            if (source == null || count < 0 || count > source.Length) throw new ArgumentOutOfRangeException("count");
            lock (gate) {
                total += count;
                if (count >= tail.Length) { Buffer.BlockCopy(source, count - tail.Length, tail, 0, tail.Length); used = tail.Length; truncated = true; return; }
                if (used + count > tail.Length) { int drop = used + count - tail.Length; Buffer.BlockCopy(tail, drop, tail, 0, used - drop); used -= drop; truncated = true; }
                Buffer.BlockCopy(source, 0, tail, used, count); used += count;
            }
        }
        public byte[] Snapshot() { lock (gate) { var value = new byte[used]; Buffer.BlockCopy(tail, 0, value, 0, used); return value; } }
        public long TotalBytes { get { lock (gate) return total; } }
        public bool Truncated { get { lock (gate) return truncated; } }
    }
    public static object SyntheticCaptureTail(string[] chunks) {
        var tail = new BoundedTail(32768);
        foreach (string chunk in chunks) tail.Append(Encoding.UTF8.GetBytes(chunk), Encoding.UTF8.GetByteCount(chunk));
        return new { Tail = tail.Snapshot(), TotalBytes = tail.TotalBytes, Truncated = tail.Truncated };
    }
    public sealed class Capture : IDisposable {
        readonly IntPtr rawHandle;
        readonly SafeFileHandle handle;
        readonly object gate = new object();
        readonly BoundedTail tail = new BoundedTail(32768);
        string error;
        public readonly ManualResetEvent Eof = new ManualResetEvent(false);
        readonly Task reader;
        bool closed;
        public Capture(IntPtr readHandle) {
            rawHandle = readHandle; handle = new SafeFileHandle(readHandle, false);
            reader = Task.Run(() => {
                try {
                    using (var stream = new FileStream(handle, FileAccess.Read, 4096, false)) {
                        byte[] chunk = new byte[8192];
                        int n;
                        while ((n = stream.Read(chunk, 0, chunk.Length)) != 0) Append(chunk, n);
                    }
                } catch (Exception e) { lock (gate) error = e.GetType().Name; }
                finally { Eof.Set(); }
            });
        }
        void Append(byte[] source, int count) {
            tail.Append(source, count);
        }
        public byte[] Tail { get { return tail.Snapshot(); } }
        public long TotalBytes { get { return tail.TotalBytes; } }
        public bool Truncated { get { return tail.Truncated; } }
        public string Error { get { lock (gate) return error; } }
        public bool WaitEof(int milliseconds) { return Eof.WaitOne(milliseconds); }
        public bool WaitReader(int milliseconds) { return reader.Wait(Math.Max(0, milliseconds)); }
        public bool ReaderComplete { get { return reader.IsCompleted; } }
        public string Text { get { return Encoding.UTF8.GetString(Tail); } }
        public bool CloseChecked(int milliseconds) {
            if (closed) return true;
            long until = Environment.TickCount64 + Math.Max(0, milliseconds);
            if (!Eof.WaitOne(Remaining(until)) || !reader.Wait(Remaining(until)) || Error != null) return false;
            handle.Dispose();
            if (!CloseHandle(rawHandle)) return false;
            closed = true;
            Eof.Dispose();
            return true;
        }
        public void Dispose() { CloseChecked(0); }
    }
    public sealed class ProcessLease {
        public IntPtr Handle;
        public IntPtr ThreadHandle;
        public IntPtr StdoutReadHandle;
        public IntPtr StdoutWriteHandle;
        public IntPtr StderrReadHandle;
        public IntPtr StderrWriteHandle;
        public IntPtr NullInputHandle;
        public uint Pid;
        public Capture Stdout;
        public Capture Stderr;
        public bool Registered;
        public bool Created;
        public bool Assigned;
        public bool ResumeAttempted;
        public bool Resumed;
        public bool ThreadCloseAttempted;
        public bool ProcessExitConfirmed;
        public bool CapturesComplete;
        public bool UnassignedTerminationAttempted;
        uint exitCode;
        public uint ExitCode { get { if (!ProcessExitConfirmed && Handle != IntPtr.Zero) Need(GetExitCodeProcess(Handle, out exitCode), "GetExitCodeProcess"); return exitCode; } set { exitCode = value; } }
        public bool RecordProcessCreated(IntPtr process, IntPtr thread, uint pid) {
            if (!Registered || Created || process == IntPtr.Zero || thread == IntPtr.Zero || pid == 0) throw new InvalidOperationException("launch_process_registration_invalid");
            Handle = process; ThreadHandle = thread; Pid = pid; Created = true; return true;
        }
        public void RecordAssigned() {
            if (!Created || Assigned) throw new InvalidOperationException("launch_job_assignment_state_invalid");
            Assigned = true;
        }
        public void BeginResume() {
            if (!Assigned || ResumeAttempted || Resumed) throw new InvalidOperationException("launch_resume_attempt_invalid");
            ResumeAttempted = true;
        }
        public void RecordResumed() {
            if (!ResumeAttempted || Resumed) throw new InvalidOperationException("launch_resume_result_invalid");
            Resumed = true;
        }
        public void BeginThreadClose() {
            if (ThreadCloseAttempted || ThreadHandle == IntPtr.Zero) throw new InvalidOperationException("launch_thread_close_attempt_invalid");
            ThreadCloseAttempted = true;
        }
        public void RecordThreadClose(bool closed) {
            if (!ThreadCloseAttempted || ThreadHandle == IntPtr.Zero) throw new InvalidOperationException("launch_thread_close_result_invalid");
            if (closed) ThreadHandle = IntPtr.Zero;
        }
        public void RecordProcessExited(uint code) {
            if (!Created || ProcessExitConfirmed) throw new InvalidOperationException("launch_process_exit_state_invalid");
            exitCode = code; ProcessExitConfirmed = true;
        }
        public void RecordCapturesSettled(bool settled) { if (settled) CapturesComplete = true; }
        public bool CanCloseHandles(bool jobZeroConfirmed) { return CapturesComplete && (!Assigned || jobZeroConfirmed) && (!Created || ProcessExitConfirmed); }
        public bool Wait(int milliseconds) { return WaitSignaledOrTimeout(Handle, milliseconds, "WaitForSingleObject(process)"); }
        public bool Alive { get { uint code; Need(GetExitCodeProcess(Handle, out code), "GetExitCodeProcess"); return code == STILL_ACTIVE && !WaitSignaledOrTimeout(Handle, 0, "WaitForSingleObject(process_liveness)"); } }
        public long CreationFileTime { get { long created, exited, kernel, user; Need(GetProcessTimes(Handle, out created, out exited, out kernel, out user), "GetProcessTimes"); return created; } }
        public string Image { get { var b = new StringBuilder(32768); int n = b.Capacity; Need(QueryFullProcessImageNameW(Handle, 0, b, ref n), "QueryFullProcessImageNameW"); return b.ToString(0, n); } }
        public string UserSid {
            get {
                IntPtr token; Need(OpenProcessToken(Handle, TOKEN_QUERY, out token), "OpenProcessToken");
                try {
                    int needed; GetTokenInformation(token, 1, IntPtr.Zero, 0, out needed);
                    if (needed <= 0 || needed > 65536) throw new InvalidOperationException("token_information_size_invalid");
                    IntPtr data = Marshal.AllocHGlobal(needed);
                    try { Need(GetTokenInformation(token, 1, data, needed, out needed), "GetTokenInformation"); return new SecurityIdentifier(Marshal.ReadIntPtr(data)).Value; }
                    finally { Marshal.FreeHGlobal(data); }
                } finally { Need(CloseHandle(token), "CloseHandle(process_token)"); }
            }
        }
        public bool CloseChecked(int milliseconds, bool jobZeroConfirmed) {
            if (!CanCloseHandles(jobZeroConfirmed)) return false;
            long until = Environment.TickCount64 + Math.Max(0, milliseconds);
            bool stdout = Stdout == null ? WindowsSccacheNative.TryCloseHandle(ref StdoutReadHandle) : Stdout.CloseChecked(Remaining(until));
            bool stderr = Stderr == null ? WindowsSccacheNative.TryCloseHandle(ref StderrReadHandle) : Stderr.CloseChecked(Remaining(until));
            bool writers = WindowsSccacheNative.TryCloseHandle(ref StdoutWriteHandle) & WindowsSccacheNative.TryCloseHandle(ref StderrWriteHandle) & WindowsSccacheNative.TryCloseHandle(ref NullInputHandle);
            bool thread = WindowsSccacheNative.TryCloseHandle(ref ThreadHandle);
            bool process = WindowsSccacheNative.TryCloseHandle(ref Handle);
            return stdout && stderr && writers && thread && process;
        }
    }
    public sealed class LaunchRegistry {
        readonly List<ProcessLease> leases = new List<ProcessLease>();
        public ProcessLease Register() { var lease = new ProcessLease { Registered = true }; leases.Add(lease); return lease; }
        public ProcessLease[] Snapshot() { return leases.ToArray(); }
        public bool Contains(ProcessLease lease) { return leases.Contains(lease); }
    }
    static bool TryCloseHandle(ref IntPtr handle) {
        if (handle == IntPtr.Zero) return true;
        if (!CloseHandle(handle)) return false;
        handle = IntPtr.Zero; return true;
    }
    public static LaunchRegistry CreateLaunchRegistry() { return new LaunchRegistry(); }
    public static ProcessLease RegisterLaunch(LaunchRegistry registry) { if (registry == null) throw new ArgumentNullException("registry"); return registry.Register(); }
    public static object[] SnapshotLaunches(LaunchRegistry registry) { return registry == null ? new object[0] : (object[])registry.Snapshot(); }
    public static bool TerminateUnassigned(ProcessLease lease) {
        if (!lease.Created || lease.Assigned || lease.Handle == IntPtr.Zero || lease.UnassignedTerminationAttempted) throw new InvalidOperationException("unassigned_child_termination_state_invalid");
        lease.UnassignedTerminationAttempted = true;
        return TerminateProcess(lease.Handle, 1);
    }
    public static bool CloseLaunchParentHandles(ProcessLease lease) {
        bool stdout = TryCloseHandle(ref lease.StdoutWriteHandle);
        bool stderr = TryCloseHandle(ref lease.StderrWriteHandle);
        bool input = TryCloseHandle(ref lease.NullInputHandle);
        return stdout && stderr && input;
    }
    public static bool CanCloseLaunchHandles(ProcessLease lease, bool jobZeroConfirmed) { return lease.CanCloseHandles(jobZeroConfirmed); }
    public static bool CloseLaunchLease(ProcessLease lease, bool jobZeroConfirmed, int milliseconds) { return lease.CloseChecked(milliseconds, jobZeroConfirmed); }
    public static object[] LaunchLeaseContractControls() {
        var results = new List<object>();
        results.Add(LaunchLeasePhaseControl("resume_failure", false));
        results.Add(LaunchThreadCloseFailureControl());
        var registry = new LaunchRegistry(); var once = registry.Register(); once.RecordProcessCreated(new IntPtr(11), new IntPtr(12), 13); once.RecordAssigned(); once.BeginResume();
        bool refused = false;
        try { once.BeginResume(); } catch (InvalidOperationException) { refused = true; }
        results.Add(new { Name = "resume_retry_refused", Passed = refused, Error = "" });
        var unresolved = new LaunchRegistry(); var open = unresolved.Register(); open.RecordProcessCreated(new IntPtr(21), new IntPtr(22), 23); open.RecordAssigned();
        results.Add(new { Name = "unsettled_job_or_process_keeps_handles", Passed = !open.CanCloseHandles(false), Error = "" });
        var processPending = new LaunchRegistry(); var noExit = processPending.Register(); noExit.RecordProcessCreated(new IntPtr(51), new IntPtr(52), 53); noExit.RecordAssigned(); noExit.RecordCapturesSettled(true);
        results.Add(new { Name = "process_exit_unconfirmed_keeps_handles", Passed = !noExit.CanCloseHandles(true), Error = "" });
        var capturePending = new LaunchRegistry(); var noCapture = capturePending.Register(); noCapture.RecordProcessCreated(new IntPtr(61), new IntPtr(62), 63); noCapture.RecordAssigned(); noCapture.RecordProcessExited(0);
        results.Add(new { Name = "capture_reader_unconfirmed_keeps_handles", Passed = !noCapture.CanCloseHandles(true), Error = "" });
        var settled = new LaunchRegistry(); var allDone = settled.Register(); allDone.RecordProcessCreated(new IntPtr(71), new IntPtr(72), 73); allDone.RecordAssigned(); allDone.RecordProcessExited(0); allDone.RecordCapturesSettled(true);
        results.Add(new { Name = "settled_job_process_and_captures_can_close", Passed = allDone.CanCloseHandles(true), Error = "" });
        return results.ToArray();
    }
    static object LaunchLeasePhaseControl(string name, bool resumeSucceeds) {
        var registry = new LaunchRegistry(); var lease = registry.Register();
        lease.RecordProcessCreated(new IntPtr(31), new IntPtr(32), 33); lease.RecordAssigned(); lease.BeginResume();
        if (resumeSucceeds) lease.RecordResumed();
        bool retained = registry.Contains(lease) && lease.Handle == new IntPtr(31) && lease.ThreadHandle == new IntPtr(32) && lease.Assigned;
        bool heldUntilJobZero = !lease.CanCloseHandles(false);
        lease.RecordProcessExited(0); lease.RecordCapturesSettled(true);
        return new { Name = name + "_failure_lease_retained_until_job_settlement", Passed = retained && heldUntilJobZero && lease.CanCloseHandles(true), Error = "" };
    }
    static object LaunchThreadCloseFailureControl() {
        var registry = new LaunchRegistry(); var lease = registry.Register();
        lease.RecordProcessCreated(new IntPtr(41), new IntPtr(42), 43); lease.RecordAssigned(); lease.BeginResume(); lease.RecordResumed(); lease.BeginThreadClose(); lease.RecordThreadClose(false);
        bool retained = registry.Contains(lease) && lease.ThreadHandle == new IntPtr(42) && lease.ResumeAttempted && lease.Resumed;
        bool retryRefused = false;
        try { lease.BeginThreadClose(); } catch (InvalidOperationException) { retryRefused = true; }
        lease.RecordProcessExited(0); lease.RecordCapturesSettled(true);
        return new { Name = "primary_thread_close_failure_handle_retained_no_retry", Passed = retained && retryRefused && lease.CanCloseHandles(true), Error = "" };
    }
    public sealed class JobLease : IDisposable {
        public readonly IntPtr Handle;
        IntPtr port;
        bool initialized;
        public JobLease() {
            Handle = CreateJobObjectW(IntPtr.Zero, null); if (Handle == IntPtr.Zero) throw Last("CreateJobObjectW");
            port = IntPtr.Zero;
        }
        public void Initialize() {
            if (initialized || Handle == IntPtr.Zero) throw new InvalidOperationException("job_initialization_state_invalid");
            try {
                port = CreateIoCompletionPort(new IntPtr(-1), IntPtr.Zero, UIntPtr.Zero, 0); if (port == IntPtr.Zero) throw Last("CreateIoCompletionPort");
                var limits = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION(); limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION)));
                try { Marshal.StructureToPtr(limits, p, false); Need(SetInformationJobObject(Handle, JobObjectExtendedLimitInformation, p, (uint)Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION))), "SetInformationJobObject(limits)"); }
                finally { Marshal.FreeHGlobal(p); }
                var assoc = new JOBOBJECT_ASSOCIATE_COMPLETION_PORT { CompletionKey = new IntPtr(1), CompletionPort = port };
                p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(JOBOBJECT_ASSOCIATE_COMPLETION_PORT)));
                try { Marshal.StructureToPtr(assoc, p, false); Need(SetInformationJobObject(Handle, JobObjectAssociateCompletionPortInformation, p, (uint)Marshal.SizeOf(typeof(JOBOBJECT_ASSOCIATE_COMPLETION_PORT))), "SetInformationJobObject(completion)"); }
                finally { Marshal.FreeHGlobal(p); }
                initialized = true;
            } catch (Exception original) {
                throw new InvalidOperationException("job_initialization_failed_cleanup_required:" + original.GetType().Name + ":" + original.Message, original);
            }
        }
        public uint ActiveCount {
            get {
                IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)));
                try { uint needed; Need(QueryInformationJobObject(Handle, JobObjectBasicAccountingInformation, p, (uint)Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)), out needed), "QueryInformationJobObject(accounting)"); return ((JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)Marshal.PtrToStructure(p, typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION))).ActiveProcesses; }
                finally { Marshal.FreeHGlobal(p); }
            }
        }
        public uint[] Members() {
            const int capacity = 8192;
            int length = 8 + capacity * IntPtr.Size;
            IntPtr p = Marshal.AllocHGlobal(length);
            try {
                uint needed; Need(QueryInformationJobObject(Handle, JobObjectBasicProcessIdList, p, (uint)length, out needed), "QueryInformationJobObject(process_ids)");
                uint assigned = unchecked((uint)Marshal.ReadInt32(p)); uint listed = unchecked((uint)Marshal.ReadInt32(p, 4));
                if (listed > capacity || assigned != listed) throw new InvalidOperationException("job_process_list_incomplete");
                var values = new uint[(int)listed];
                for (int i = 0; i < (int)listed; i++) values[i] = unchecked((uint)Marshal.ReadIntPtr(p, 8 + i * IntPtr.Size).ToInt64());
                return values;
            } finally { Marshal.FreeHGlobal(p); }
        }
        public bool Contains(IntPtr process) { bool result; Need(IsProcessInJob(process, Handle, out result), "IsProcessInJob"); return result; }
        public bool Terminate(uint exitCode) { return TerminateJobObject(Handle, exitCode); }
        public bool WaitForZero(int milliseconds) {
            if (!initialized || port == IntPtr.Zero) throw new InvalidOperationException("job_completion_port_unavailable");
            long until = Environment.TickCount64 + Math.Max(0, milliseconds);
            while (true) {
                int left = Remaining(until); if (left == 0) return false;
                uint bytes; UIntPtr key; IntPtr overlapped;
                bool ok = GetQueuedCompletionStatus(port, out bytes, out key, out overlapped, (uint)left);
                if (!ok) { int err = Marshal.GetLastWin32Error(); if (overlapped == IntPtr.Zero && err == 258) return false; throw new Win32Exception(err, "GetQueuedCompletionStatus"); }
                if (key == new UIntPtr(1) && bytes == JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO && ActiveCount == 0) return true;
            }
        }
        public bool CloseChecked() { bool a = port == IntPtr.Zero || CloseHandle(port); bool b = Handle == IntPtr.Zero || CloseHandle(Handle); return a && b; }
        public void Dispose() { CloseChecked(); }
    }
    public static JobLease CreateJob() { return new JobLease(); }
    public static void InitializeJob(JobLease job) { if (job == null) throw new ArgumentNullException("job"); job.Initialize(); }
    public static void CreateDirectoryNew(string path) { Need(CreateDirectoryW(path, IntPtr.Zero), "CreateDirectoryW"); }
    static void CloseLeaseHandle(ref IntPtr handle, string op) {
        if (handle == IntPtr.Zero) return;
        Need(CloseHandle(handle), op); handle = IntPtr.Zero;
    }
    public static void Launch(IntPtr job, string app, string commandLine, string cwd, IDictionary<string,string> env, ProcessLease lease) {
        if (lease == null || !lease.Registered || lease.Created) throw new InvalidOperationException("launch_lease_not_registered_before_start");
        SECURITY_ATTRIBUTES sa = new SECURITY_ATTRIBUTES { nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)), bInheritHandle = 1 };
        IntPtr attrs = IntPtr.Zero, handleArray = IntPtr.Zero, list = IntPtr.Zero;
        PROCESS_INFORMATION pi = new PROCESS_INFORMATION();
        try {
            IntPtr readHandle, writeHandle;
            Need(CreatePipe(out readHandle, out writeHandle, ref sa, 0), "CreatePipe(stdout)"); lease.StdoutReadHandle = readHandle; lease.StdoutWriteHandle = writeHandle;
            Need(CreatePipe(out readHandle, out writeHandle, ref sa, 0), "CreatePipe(stderr)"); lease.StderrReadHandle = readHandle; lease.StderrWriteHandle = writeHandle;
            Need(SetHandleInformation(lease.StdoutReadHandle, HANDLE_FLAG_INHERIT, 0), "SetHandleInformation(stdout)");
            Need(SetHandleInformation(lease.StderrReadHandle, HANDLE_FLAG_INHERIT, 0), "SetHandleInformation(stderr)");
            lease.Stdout = new Capture(lease.StdoutReadHandle); lease.StdoutReadHandle = IntPtr.Zero;
            lease.Stderr = new Capture(lease.StderrReadHandle); lease.StderrReadHandle = IntPtr.Zero;
            lease.NullInputHandle = CreateFileW("NUL", GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, ref sa, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, IntPtr.Zero);
            if (lease.NullInputHandle == new IntPtr(-1)) { lease.NullInputHandle = IntPtr.Zero; throw Last("CreateFileW(NUL)"); }
            var handles = new[] { lease.NullInputHandle, lease.StdoutWriteHandle, lease.StderrWriteHandle };
            IntPtr attrSize = IntPtr.Zero; InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attrSize);
            if (attrSize == IntPtr.Zero || attrSize.ToInt64() > 65536) throw new InvalidOperationException("process_attribute_size_invalid");
            attrs = Marshal.AllocHGlobal(attrSize); handleArray = Marshal.AllocHGlobal(handles.Length * IntPtr.Size);
            Need(InitializeProcThreadAttributeList(attrs, 1, 0, ref attrSize), "InitializeProcThreadAttributeList"); list = attrs;
            for (int i = 0; i < handles.Length; i++) Marshal.WriteIntPtr(handleArray, i * IntPtr.Size, handles[i]);
            Need(UpdateProcThreadAttribute(list, 0, new IntPtr(PROC_THREAD_ATTRIBUTE_HANDLE_LIST), handleArray, new IntPtr(handles.Length * IntPtr.Size), IntPtr.Zero, IntPtr.Zero), "UpdateProcThreadAttribute");
            var si = new STARTUPINFOEX(); si.lpAttributeList = list; si.StartupInfo.cb = Marshal.SizeOf(typeof(STARTUPINFOEX)); si.StartupInfo.dwFlags = STARTF_USESTDHANDLES; si.StartupInfo.hStdInput = lease.NullInputHandle; si.StartupInfo.hStdOutput = lease.StdoutWriteHandle; si.StartupInfo.hStdError = lease.StderrWriteHandle;
            var envText = new StringBuilder(); foreach (var pair in env) envText.Append(pair.Key).Append('=').Append(pair.Value).Append('\0'); envText.Append('\0');
            var mutableCommand = new StringBuilder(commandLine);
            Need(CreateProcessW(app, mutableCommand, IntPtr.Zero, IntPtr.Zero, true, CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT, envText, cwd, ref si, out pi), "CreateProcessW");
            lease.RecordProcessCreated(pi.hProcess, pi.hThread, pi.dwProcessId);
            Need(AssignProcessToJobObject(job, lease.Handle), "AssignProcessToJobObject");
            lease.RecordAssigned();
            CloseLeaseHandle(ref lease.StdoutWriteHandle, "CloseHandle(stdout_write)");
            CloseLeaseHandle(ref lease.StderrWriteHandle, "CloseHandle(stderr_write)");
            CloseLeaseHandle(ref lease.NullInputHandle, "CloseHandle(stdin_null)");
            lease.BeginResume();
            uint previousSuspendCount = ResumeThread(lease.ThreadHandle);
            if (previousSuspendCount == UInt32.MaxValue) throw Last("ResumeThread");
            if (previousSuspendCount != 1) throw new InvalidOperationException("primary_thread_suspend_count_unexpected");
            lease.RecordResumed();
            lease.BeginThreadClose();
            bool threadClosed = CloseHandle(lease.ThreadHandle);
            lease.RecordThreadClose(threadClosed);
            if (!threadClosed) throw Last("CloseHandle(primary_thread)");
        } catch (Exception original) {
            if (lease.Assigned) throw new InvalidOperationException("assigned_child_launch_failed_job_cleanup_required:" + original.GetType().Name + ":" + original.Message, original);
            if (lease.Created) throw new InvalidOperationException("unassigned_child_launch_cleanup_required:" + original.GetType().Name + ":" + original.Message, original);
            throw;
        } finally {
            if (list != IntPtr.Zero) DeleteProcThreadAttributeList(list);
            if (attrs != IntPtr.Zero) Marshal.FreeHGlobal(attrs); if (handleArray != IntPtr.Zero) Marshal.FreeHGlobal(handleArray);
        }
    }
    public static bool InJob(IntPtr process, IntPtr job) { bool result; Need(IsProcessInJob(process, job, out result), "IsProcessInJob"); return result; }
    public static IntPtr OpenProcessHandle(uint pid) { IntPtr p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, false, pid); if (p == IntPtr.Zero) throw Last("OpenProcess"); return p; }
    public static long ProcessCreation(IntPtr p) { long created, exited, kernel, user; Need(GetProcessTimes(p, out created, out exited, out kernel, out user), "GetProcessTimes"); return created; }
    public static string ProcessImage(IntPtr p) { var b = new StringBuilder(32768); int n = b.Capacity; Need(QueryFullProcessImageNameW(p, 0, b, ref n), "QueryFullProcessImageNameW"); return b.ToString(0, n); }
    public static string ProcessUserSid(IntPtr p) {
        IntPtr token; Need(OpenProcessToken(p, TOKEN_QUERY, out token), "OpenProcessToken");
        try { int needed; GetTokenInformation(token, 1, IntPtr.Zero, 0, out needed); if (needed <= 0 || needed > 65536) throw new InvalidOperationException("token_information_size_invalid"); IntPtr data = Marshal.AllocHGlobal(needed); try { Need(GetTokenInformation(token, 1, data, needed, out needed), "GetTokenInformation"); return new SecurityIdentifier(Marshal.ReadIntPtr(data)).Value; } finally { Marshal.FreeHGlobal(data); } }
        finally { Need(CloseHandle(token), "CloseHandle(process_token)"); }
    }
    public static bool HandleAlive(IntPtr p) { uint code; Need(GetExitCodeProcess(p, out code), "GetExitCodeProcess"); return code == STILL_ACTIVE && !WaitSignaledOrTimeout(p, 0, "WaitForSingleObject(handle_liveness)"); }
    public static uint ExitCodeHandle(IntPtr p) { uint code; Need(GetExitCodeProcess(p, out code), "GetExitCodeProcess"); return code; }
    public static bool WaitHandle(IntPtr p, int ms) { return WaitSignaledOrTimeout(p, ms, "WaitForSingleObject(handle)"); }
    public static bool WaitCapture(Capture c, int ms) { return c.WaitEof(ms); }
    public static object CaptureFacts(Capture c) { return new { Tail = c.Tail, TotalBytes = c.TotalBytes, Truncated = c.Truncated, Error = c.Error, Eof = c.Eof.WaitOne(0), ReaderComplete = c.ReaderComplete }; }
    public sealed class TcpListenerFact {
        public readonly int Family;
        public readonly string Address;
        public readonly int Port;
        public readonly uint Pid;
        public TcpListenerFact(int family, string address, int port, uint pid) { Family = family; Address = address; Port = port; Pid = pid; }
    }
    static int NetworkPort(byte[] row, int offset) { return (row[offset] << 8) | row[offset + 1]; }
    public static TcpListenerFact[] DecodeTcpTable(byte[] table, int family, int requestedPort) {
        int rowSize = family == 4 ? 24 : family == 6 ? 56 : 0;
        if (rowSize == 0 || table == null || table.Length < 4 || requestedPort < 0 || requestedPort > 65535) throw new InvalidOperationException("tcp_table_header_invalid");
        uint count = BitConverter.ToUInt32(table, 0);
        if (count > (uint)((table.Length - 4) / rowSize)) throw new InvalidOperationException(family == 4 ? "tcp_v4_table_invalid" : "tcp_v6_table_invalid");
        var found = new List<TcpListenerFact>();
        for (int i = 0; i < (int)count; i++) {
            int offset = 4 + i * rowSize;
            if (family == 4) {
                uint state = BitConverter.ToUInt32(table, offset);
                int port = NetworkPort(table, offset + 8);
                if (state != 2 || port != requestedPort) continue;
                var address = new byte[4]; Buffer.BlockCopy(table, offset + 4, address, 0, 4);
                found.Add(new TcpListenerFact(4, new IPAddress(address).ToString(), port, BitConverter.ToUInt32(table, offset + 20)));
            } else {
                int port = NetworkPort(table, offset + 20);
                uint state = BitConverter.ToUInt32(table, offset + 48);
                if (state != 2 || port != requestedPort) continue;
                var address = new byte[16]; Buffer.BlockCopy(table, offset, address, 0, 16);
                found.Add(new TcpListenerFact(6, new IPAddress(address, BitConverter.ToUInt32(table, offset + 16)).ToString(), port, BitConverter.ToUInt32(table, offset + 52)));
            }
        }
        return found.ToArray();
    }
    static byte[] TcpTable(int family) {
        int size = 1024 * 1024; byte[] data = new byte[size]; int result = GetExtendedTcpTable(data, ref size, false, family, 3, 0);
        if (result != 0) throw new Win32Exception(result, "GetExtendedTcpTable");
        if (size < 4 || size > data.Length) throw new InvalidOperationException("tcp_table_size_invalid");
        Array.Resize(ref data, size); return data;
    }
    public static TcpListenerFact[] Listeners(int port) {
        var found = new List<TcpListenerFact>();
        found.AddRange(DecodeTcpTable(TcpTable(2), 4, port));
        found.AddRange(DecodeTcpTable(TcpTable(23), 6, port));
        return found.ToArray();
    }
    static void PutUInt32(byte[] row, int offset, uint value) { Buffer.BlockCopy(BitConverter.GetBytes(value), 0, row, offset, 4); }
    static byte[] V4Row(byte[] address, byte highPort, byte lowPort, uint pid) {
        var row = new byte[24]; PutUInt32(row, 0, 2); Buffer.BlockCopy(address, 0, row, 4, 4); row[8] = highPort; row[9] = lowPort; PutUInt32(row, 20, pid); return row;
    }
    static byte[] V4Table(params byte[][] rows) {
        var table = new byte[4 + rows.Length * 24]; PutUInt32(table, 0, (uint)rows.Length);
        for (int i = 0; i < rows.Length; i++) Buffer.BlockCopy(rows[i], 0, table, 4 + i * 24, 24);
        return table;
    }
    static byte[] V6Table() {
        var row = new byte[56]; row[15] = 1; row[20] = 0x10; row[21] = 0x82; PutUInt32(row, 48, 2); PutUInt32(row, 52, 126);
        var table = new byte[60]; PutUInt32(table, 0, 1); Buffer.BlockCopy(row, 0, table, 4, row.Length); return table;
    }
    static object PureControl(string name, Func<bool> assertion) {
        try { return new { Name = name, Passed = assertion(), Error = "" }; }
        catch (Exception error) { return new { Name = name, Passed = false, Error = error.GetType().Name }; }
    }
    public static object[] TcpDecoderContractControls() {
        byte[] loopback = V4Row(new byte[] { 127, 0, 0, 1 }, 0x10, 0x82, 123);
        byte[] wrongPort = V4Row(new byte[] { 127, 0, 0, 1 }, 0x10, 0x83, 124);
        byte[] wildcard = V4Row(new byte[] { 0, 0, 0, 0 }, 0x10, 0x82, 125);
        byte[] foreign = V4Row(new byte[] { 192, 0, 2, 10 }, 0x10, 0x82, 126);
        var results = new List<object>();
        results.Add(PureControl("ipv4_loopback_port_pid", () => { var rows = DecodeTcpTable(V4Table(loopback), 4, 4226); return rows.Length == 1 && rows[0].Family == 4 && rows[0].Address == "127.0.0.1" && rows[0].Port == 4226 && rows[0].Pid == 123; }));
        results.Add(PureControl("ipv4_wrong_port_excluded", () => DecodeTcpTable(V4Table(wrongPort), 4, 4226).Length == 0));
        results.Add(PureControl("ipv4_wildcard_visible", () => { var rows = DecodeTcpTable(V4Table(wildcard), 4, 4226); return rows.Length == 1 && rows[0].Address == "0.0.0.0" && rows[0].Pid == 125; }));
        results.Add(PureControl("ipv4_foreign_address_visible", () => { var rows = DecodeTcpTable(V4Table(foreign), 4, 4226); return rows.Length == 1 && rows[0].Address == "192.0.2.10" && rows[0].Pid == 126; }));
        results.Add(PureControl("ipv6_loopback_port_pid", () => { var rows = DecodeTcpTable(V6Table(), 6, 4226); return rows.Length == 1 && rows[0].Family == 6 && rows[0].Address == "::1" && rows[0].Port == 4226 && rows[0].Pid == 126; }));
        results.Add(PureControl("incomplete_ipv4_table_refused", () => { byte[] table = V4Table(loopback); PutUInt32(table, 0, 2); try { DecodeTcpTable(table, 4, 4226); return false; } catch (InvalidOperationException error) { return error.Message == "tcp_v4_table_invalid"; } }));
        return results.ToArray();
    }
    public static bool WaitJobZero(JobLease job, int ms) { return job.WaitForZero(ms); }
    public static uint ActiveCount(JobLease job) { return job.ActiveCount; }
    public static uint[] Members(JobLease job) { return job.Members(); }
    public static object Snapshot(JobLease job) { uint[] members = job.Members(); return new { ActiveCount = (uint)members.Length, Members = members }; }
    public static bool Contains(JobLease job, IntPtr process) { return job.Contains(process); }
    public static bool Terminate(JobLease job, uint code) { return job.Terminate(code); }
    public static bool CloseHandlePublic(IntPtr handle) { return handle != IntPtr.Zero && CloseHandle(handle); }
}
