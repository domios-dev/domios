using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
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
    const uint PIPE_ACCESS_DUPLEX = 3;
    const uint FILE_FLAG_OVERLAPPED = 0x40000000;
    const uint FILE_FLAG_FIRST_PIPE_INSTANCE = 0x00080000;
    const uint PIPE_REJECT_REMOTE_CLIENTS = 8;
    const uint STARTUP_FRAME_LIMIT = 4096;
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
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern IntPtr CreateNamedPipeW(string name, uint openMode, uint pipeMode, uint maxInstances, uint outBufferSize, uint inBufferSize, uint defaultTimeout, IntPtr attrs);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetNamedPipeClientProcessId(IntPtr pipe, out uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern IntPtr CreateEventW(IntPtr attrs, bool manualReset, bool initialState, string name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetEvent(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForMultipleObjects(uint count, IntPtr[] handles, bool waitAll, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern bool CreateProcessW(string app, StringBuilder command, IntPtr procAttrs, IntPtr threadAttrs, bool inherit, uint flags, StringBuilder environment, string cwd, ref STARTUPINFOEX startup, out PROCESS_INFORMATION info);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)] static extern bool CreateDirectoryW(string path, IntPtr securityAttributes);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint GetConsoleCP();
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint GetConsoleProcessList([Out] uint[] processes, uint capacity);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetProcessTimes(IntPtr process, out long creation, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint GetProcessId(IntPtr process);
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
        public bool DirectServer;
        public uint ParentConsoleCodePage;
        public bool Assigned;
        public bool ResumeAttempted;
        public bool Resumed;
        public bool ThreadCloseAttempted;
        public bool ProcessExitConfirmed;
        public bool CapturesComplete;
        public bool UnassignedTerminationAttempted;
        public long CreatedFileTime;
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
        public void RecordCreationFileTime(long value) {
            if (!Created || value <= 0 || CreatedFileTime != 0) throw new InvalidOperationException("launch_creation_identity_state_invalid");
            CreatedFileTime = value;
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
    public sealed class StartupFrameResult {
        public bool Valid;
        public string Status;
        public string Address;
        public string Error;
        public uint PayloadBytes;
    }
    public sealed class StartupNoticeEvidence {
        public string PipePath;
        public bool Connected;
        public bool ReaderComplete;
        public string ReaderError;
        public uint ClientPid;
        public uint ServerPid;
        public uint HandlePidBefore;
        public uint HandlePidAfter;
        public long CapturedCreationFileTime;
        public long CreationFileTimeBefore;
        public long CreationFileTimeAfter;
        public bool HandleRetained;
        public bool JobMembershipBefore;
        public bool JobMembershipAfter;
        public bool AliveBefore;
        public bool AliveAfter;
        public StartupFrameResult Frame;
        public bool Valid;
        public string[] Reasons;
    }
    public sealed class DirectServerDecision {
        public bool Valid;
        public string[] Reasons;
    }
    public sealed class StartupNotificationLease {
        public string PipePath;
        public IntPtr RawPipeHandle;
        public SafePipeHandle SafePipeHandle;
        public NamedPipeServerStream Pipe;
        public IntPtr CompletionHandle;
        public CancellationTokenSource Cancellation;
        public Task Reader;
        public Task ReaderCompletion;
        public byte[] Frame;
        public StartupFrameResult Decoded;
        public string ReaderError;
        public uint ClientPid;
        public bool Connected;
        public bool ReaderComplete;
        public bool ReaderStarted;
        public bool Closed;
        public long StartedTicks;
        public long CompletedTicks;
    }
    public static StartupNotificationLease CreateStartupNotificationLease() {
        return new StartupNotificationLease { PipePath = @"\\.\pipe\sccache-startup-" + Guid.NewGuid().ToString("N") };
    }
    public static string OpenStartupNotificationPipe(StartupNotificationLease lease) {
        if (lease == null || lease.Closed || lease.RawPipeHandle != IntPtr.Zero || lease.SafePipeHandle != null) throw new InvalidOperationException("startup_pipe_lease_invalid");
        IntPtr handle = CreateNamedPipeW(lease.PipePath, PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_REJECT_REMOTE_CLIENTS, 1, 8192, 8192, 0, IntPtr.Zero);
        lease.RawPipeHandle = handle;
        if (handle == new IntPtr(-1)) { lease.RawPipeHandle = IntPtr.Zero; throw Last("CreateNamedPipeW(startup)"); }
        lease.CompletionHandle = CreateEventW(IntPtr.Zero, true, false, null);
        if (lease.CompletionHandle == IntPtr.Zero) throw Last("CreateEventW(startup_completion)");
        lease.SafePipeHandle = new SafePipeHandle(handle, true);
        lease.RawPipeHandle = IntPtr.Zero;
        lease.Pipe = new NamedPipeServerStream(PipeDirection.InOut, true, false, lease.SafePipeHandle);
        return lease.PipePath;
    }
    public static IDictionary<string,string> StartupServerEnvironment(IDictionary<string,string> source, string pipePath) {
        if (source == null || String.IsNullOrEmpty(pipePath)) throw new ArgumentException("startup_server_environment_invalid");
        var result = new SortedDictionary<string,string>(source, StringComparer.OrdinalIgnoreCase);
        result["SCCACHE_START_SERVER"] = "1";
        result["SCCACHE_STARTUP_NOTIFY"] = pipePath;
        return result;
    }
    public static void BeginStartupNotificationRead(StartupNotificationLease lease) {
        if (lease == null || lease.Pipe == null || lease.CompletionHandle == IntPtr.Zero || lease.ReaderStarted || lease.Closed) throw new InvalidOperationException("startup_reader_registration_invalid");
        lease.Cancellation = new CancellationTokenSource();
        lease.StartedTicks = System.Diagnostics.Stopwatch.GetTimestamp();
        lease.ReaderStarted = true;
        lease.Reader = ReadStartupNotificationAsync(lease);
        lease.ReaderCompletion = lease.Reader.ContinueWith(task => {
            lease.ReaderComplete = true;
            lease.CompletedTicks = System.Diagnostics.Stopwatch.GetTimestamp();
            if (!SetEvent(lease.CompletionHandle)) lease.ReaderError = "startup_reader_completion_signal_failed:" + Marshal.GetLastWin32Error();
        }, CancellationToken.None, TaskContinuationOptions.ExecuteSynchronously, TaskScheduler.Default);
    }
    static async Task ReadExactlyAsync(Stream stream, byte[] buffer, CancellationToken cancellation) {
        int offset = 0;
        while (offset < buffer.Length) {
            int read = await stream.ReadAsync(buffer, offset, buffer.Length - offset, cancellation).ConfigureAwait(false);
            if (read == 0) throw new EndOfStreamException("startup_frame_truncated");
            offset += read;
        }
    }
    static async Task<byte[]> ReadStartupFrameAsync(Stream stream, CancellationToken cancellation) {
        var header = new byte[4];
        await ReadExactlyAsync(stream, header, cancellation).ConfigureAwait(false);
        uint payloadBytes = ((uint)header[0] << 24) | ((uint)header[1] << 16) | ((uint)header[2] << 8) | header[3];
        if (payloadBytes < 4 || payloadBytes > STARTUP_FRAME_LIMIT) throw new InvalidDataException("startup_frame_length_invalid");
        var frame = new byte[checked(4 + (int)payloadBytes)];
        Buffer.BlockCopy(header, 0, frame, 0, 4);
        await ReadExactlyAsync(stream, frame, 4, (int)payloadBytes, cancellation).ConfigureAwait(false);
        var extra = new byte[1];
        if (await stream.ReadAsync(extra, 0, 1, cancellation).ConfigureAwait(false) != 0) throw new InvalidDataException("startup_frame_trailing_bytes");
        return frame;
    }
    static async Task ReadExactlyAsync(Stream stream, byte[] buffer, int offset, int count, CancellationToken cancellation) {
        int readOffset = 0;
        while (readOffset < count) {
            int read = await stream.ReadAsync(buffer, offset + readOffset, count - readOffset, cancellation).ConfigureAwait(false);
            if (read == 0) throw new EndOfStreamException("startup_frame_truncated");
            readOffset += read;
        }
    }
    static async Task ReadStartupNotificationAsync(StartupNotificationLease lease) {
        try {
            await lease.Pipe.WaitForConnectionAsync(lease.Cancellation.Token).ConfigureAwait(false);
            lease.Connected = true;
            Need(GetNamedPipeClientProcessId(lease.Pipe.SafePipeHandle.DangerousGetHandle(), out lease.ClientPid), "GetNamedPipeClientProcessId(startup)");
            lease.Frame = await ReadStartupFrameAsync(lease.Pipe, lease.Cancellation.Token).ConfigureAwait(false);
            lease.Decoded = DecodeStartupFrame(lease.Frame);
            if (!lease.Decoded.Valid) lease.ReaderError = lease.Decoded.Error;
        } catch (OperationCanceledException) { lease.ReaderError = "startup_reader_cancelled"; }
        catch (Exception error) {
            var win32 = error as Win32Exception;
            lease.ReaderError = win32 == null ? error.GetType().Name + ":" + error.Message : error.GetType().Name + ":" + win32.NativeErrorCode;
        }
    }
    static ulong ReadUInt64LittleEndian(byte[] bytes, int offset) {
        ulong value = 0;
        for (int i = 0; i < 8; i++) value |= (ulong)bytes[offset + i] << (i * 8);
        return value;
    }
    static string ReadBincodeString(byte[] payload, ref int offset) {
        if (payload.Length - offset < 8) throw new InvalidDataException("startup_bincode_string_length_incomplete");
        ulong length = ReadUInt64LittleEndian(payload, offset); offset += 8;
        if (length > STARTUP_FRAME_LIMIT || length > (ulong)(payload.Length - offset)) throw new InvalidDataException("startup_bincode_string_length_invalid");
        string value = new UTF8Encoding(false, true).GetString(payload, offset, (int)length);
        offset += (int)length;
        if (offset != payload.Length) throw new InvalidDataException("startup_bincode_trailing_bytes");
        return value;
    }
    public static StartupFrameResult DecodeStartupFrame(byte[] frame) {
        var result = new StartupFrameResult { PayloadBytes = (uint)Math.Max(0, (frame == null ? 0 : frame.Length) - 4) };
        try {
            if (frame == null || frame.Length < 4) throw new InvalidDataException("startup_frame_header_incomplete");
            uint payloadBytes = ((uint)frame[0] << 24) | ((uint)frame[1] << 16) | ((uint)frame[2] << 8) | frame[3];
            if (payloadBytes < 4 || payloadBytes > STARTUP_FRAME_LIMIT) throw new InvalidDataException("startup_frame_length_invalid");
            if (frame.Length != 4 + payloadBytes) throw new InvalidDataException("startup_frame_size_mismatch");
            var payload = new byte[(int)payloadBytes]; Buffer.BlockCopy(frame, 4, payload, 0, payload.Length);
            uint variant = (uint)(payload[0] | (payload[1] << 8) | (payload[2] << 16) | (payload[3] << 24));
            if (variant == 0) { int offset = 4; result.Address = ReadBincodeString(payload, ref offset); result.Status = "Ok"; }
            else if (variant == 1 || variant == 2) {
                if (payload.Length != 4) throw new InvalidDataException("startup_bincode_unit_variant_size_invalid");
                result.Status = variant == 1 ? "AddrInUse" : "TimedOut";
            } else if (variant == 3) {
                int offset = 4; ReadBincodeString(payload, ref offset); result.Status = "Err";
            } else { throw new InvalidDataException("startup_bincode_variant_unknown"); }
            result.Valid = true;
        } catch (Exception error) { result.Valid = false; result.Error = error.Message; }
        return result;
    }
    public static DirectServerDecision ClassifyDirectStartupNotice(StartupNoticeEvidence evidence) {
        var reasons = new List<string>();
        if (evidence == null) reasons.Add("startup_notice_missing");
        else {
            if (!evidence.Connected) reasons.Add("startup_pipe_connection_unconfirmed");
            if (!evidence.ReaderComplete) reasons.Add("startup_reader_completion_unconfirmed");
            if (!String.IsNullOrEmpty(evidence.ReaderError)) reasons.Add("startup_reader_error");
            if (evidence.Frame == null || !evidence.Frame.Valid) reasons.Add("startup_frame_invalid");
            else {
                if (evidence.Frame.Status != "Ok") reasons.Add("startup_reply_not_success:" + evidence.Frame.Status);
                if (evidence.Frame.Address != "127.0.0.1:4226") reasons.Add("startup_reply_endpoint_mismatch");
            }
            if (!evidence.HandleRetained || evidence.ServerPid == 0 || evidence.ClientPid != evidence.ServerPid || evidence.HandlePidBefore != evidence.ServerPid || evidence.HandlePidAfter != evidence.ServerPid) reasons.Add("startup_sender_process_identity_mismatch");
            if (evidence.CapturedCreationFileTime <= 0 || evidence.CreationFileTimeBefore != evidence.CapturedCreationFileTime || evidence.CreationFileTimeAfter != evidence.CapturedCreationFileTime) reasons.Add("startup_process_creation_identity_mismatch");
            if (!evidence.JobMembershipBefore || !evidence.JobMembershipAfter) reasons.Add("startup_process_job_membership_unconfirmed");
            if (!evidence.AliveBefore || !evidence.AliveAfter) reasons.Add("startup_process_liveness_unconfirmed");
        }
        return new DirectServerDecision { Valid = reasons.Count == 0, Reasons = UniqueReasons(reasons) };
    }
    public static DirectServerDecision ClassifyDirectServerReadiness(StartupNoticeEvidence evidence, uint[] candidatePids, bool membershipValid, bool inventoryComplete, bool candidateProofValid) {
        var reasons = new List<string>();
        DirectServerDecision startup = ClassifyDirectStartupNotice(evidence);
        reasons.AddRange(startup.Reasons);
        if (!membershipValid) reasons.Add("direct_server_job_membership_refused");
        if (!inventoryComplete) reasons.Add("direct_server_candidate_inventory_incomplete");
        if (candidatePids == null || candidatePids.Length != 1) reasons.Add("direct_server_candidate_count_not_one");
        else if (evidence == null || candidatePids[0] != evidence.ServerPid) reasons.Add("direct_server_candidate_identity_mismatch");
        if (!candidateProofValid) reasons.Add("direct_server_image_hash_account_or_listener_unproven");
        return new DirectServerDecision { Valid = reasons.Count == 0, Reasons = UniqueReasons(reasons) };
    }
    public static StartupNoticeEvidence WaitForStartupNotification(StartupNotificationLease lease, ProcessLease server, JobLease job, int milliseconds) {
        var evidence = new StartupNoticeEvidence { PipePath = lease == null ? null : lease.PipePath, Frame = lease == null ? null : lease.Decoded };
        long until = Environment.TickCount64 + Math.Max(0, milliseconds);
        try {
            if (lease == null || server == null || job == null || server.Handle == IntPtr.Zero || lease.Reader == null || lease.CompletionHandle == IntPtr.Zero) throw new InvalidOperationException("startup_wait_resources_unregistered");
            evidence.ServerPid = server.Pid; evidence.CapturedCreationFileTime = server.CreatedFileTime; evidence.HandleRetained = true;
            evidence.HandlePidBefore = HandleProcessId(server.Handle); evidence.CreationFileTimeBefore = ProcessCreation(server.Handle); evidence.JobMembershipBefore = job.Contains(server.Handle); evidence.AliveBefore = server.Alive;
            uint wait = WaitForMultipleObjects(2, new[] { server.Handle, lease.CompletionHandle }, false, (uint)Remaining(until));
            if (wait == WAIT_OBJECT_0) throw new InvalidOperationException("direct_server_exited_before_startup_notification");
            if (wait == WAIT_TIMEOUT) throw new TimeoutException("direct_server_startup_notification_deadline_expired");
            if (wait == WAIT_FAILED) throw Last("WaitForMultipleObjects(startup)");
            if (wait != WAIT_OBJECT_0 + 1) throw new InvalidOperationException("startup_notification_wait_result_invalid");
            if (lease.ReaderCompletion == null || !lease.ReaderCompletion.Wait(Remaining(until))) throw new TimeoutException("startup_reader_completion_deadline_expired");
            evidence.Connected = lease.Connected; evidence.ReaderComplete = lease.ReaderComplete; evidence.ReaderError = lease.ReaderError; evidence.ClientPid = lease.ClientPid; evidence.Frame = lease.Decoded;
            evidence.HandlePidAfter = HandleProcessId(server.Handle); evidence.CreationFileTimeAfter = ProcessCreation(server.Handle); evidence.JobMembershipAfter = job.Contains(server.Handle); evidence.AliveAfter = server.Alive;
        } catch (Exception error) {
            var win32 = error as Win32Exception;
            evidence.ReaderError = win32 == null ? error.GetType().Name + ":" + error.Message : error.GetType().Name + ":" + win32.NativeErrorCode;
            if (lease != null) { evidence.Connected = lease.Connected; evidence.ReaderComplete = lease.ReaderComplete; evidence.ClientPid = lease.ClientPid; evidence.Frame = lease.Decoded; }
        }
        DirectServerDecision decision = ClassifyDirectStartupNotice(evidence);
        evidence.Valid = decision.Valid; evidence.Reasons = decision.Reasons;
        return evidence;
    }
    public static bool CancelStartupNotification(StartupNotificationLease lease) {
        if (lease == null || lease.Closed) return true;
        bool cancelled = true;
        try { if (lease.Cancellation != null) lease.Cancellation.Cancel(); } catch { cancelled = false; }
        try { if (lease.Pipe != null) lease.Pipe.Dispose(); else if (lease.SafePipeHandle != null) lease.SafePipeHandle.Dispose(); else if (!TryCloseHandle(ref lease.RawPipeHandle)) cancelled = false; } catch { cancelled = false; }
        if (lease.SafePipeHandle != null && !lease.SafePipeHandle.IsClosed) cancelled = false;
        return cancelled;
    }
    public static bool CloseStartupNotification(StartupNotificationLease lease, int milliseconds) {
        if (lease == null || lease.Closed) return true;
        long until = Environment.TickCount64 + Math.Max(0, milliseconds); bool stream = CancelStartupNotification(lease);
        if (lease.RawPipeHandle != IntPtr.Zero && !TryCloseHandle(ref lease.RawPipeHandle)) stream = false;
        if (lease.Reader != null && !lease.Reader.Wait(Remaining(until))) return false;
        if (lease.ReaderCompletion != null && !lease.ReaderCompletion.Wait(Remaining(until))) return false;
        bool completed = !lease.ReaderStarted || lease.ReaderComplete;
        bool eventClosed = TryCloseHandle(ref lease.CompletionHandle);
        try { if (lease.Cancellation != null) lease.Cancellation.Dispose(); } catch { completed = false; }
        lease.Closed = stream && completed && eventClosed;
        return lease.Closed;
    }
    static bool TryCloseHandle(ref IntPtr handle) {
        if (handle == IntPtr.Zero) return true;
        if (!CloseHandle(handle)) return false;
        handle = IntPtr.Zero; return true;
    }
    public static LaunchRegistry CreateLaunchRegistry() { return new LaunchRegistry(); }
    public static ProcessLease RegisterLaunch(LaunchRegistry registry) { if (registry == null) throw new ArgumentNullException("registry"); return registry.Register(); }
    public static ProcessLease RegisterDirectServerLaunch(LaunchRegistry registry) { var lease = RegisterLaunch(registry); lease.DirectServer = true; return lease; }
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
    public static uint HandleProcessId(IntPtr process) { uint pid = GetProcessId(process); if (pid == 0) throw Last("GetProcessId"); return pid; }
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
        public JobProcessSnapshot Snapshot() {
            const int capacity = 8192;
            int length = 8 + capacity * IntPtr.Size;
            int accountingSize = Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION));
            var snapshot = new JobProcessSnapshot { BufferCapacity = (uint)capacity, BufferBytes = (uint)length, AccountingRequiredBytes = (uint)accountingSize, ProcessIds = new uint[0] };
            IntPtr accounting = Marshal.AllocHGlobal(accountingSize);
            try {
                uint returned;
                snapshot.AccountingStartedTicks = Stopwatch.GetTimestamp();
                if (QueryInformationJobObject(Handle, JobObjectBasicAccountingInformation, accounting, (uint)accountingSize, out returned)) {
                    snapshot.AccountingQuerySucceeded = true;
                    snapshot.AccountingReturnLength = returned;
                    snapshot.AccountingComplete = returned == (uint)accountingSize;
                    snapshot.AccountingActiveProcesses = ((JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)Marshal.PtrToStructure(accounting, typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION))).ActiveProcesses;
                } else snapshot.AccountingError = Marshal.GetLastWin32Error();
                snapshot.AccountingCompletedTicks = Stopwatch.GetTimestamp();
            } finally { Marshal.FreeHGlobal(accounting); }
            IntPtr processIds = Marshal.AllocHGlobal(length);
            try {
                uint returned;
                snapshot.ProcessListStartedTicks = Stopwatch.GetTimestamp();
                if (!QueryInformationJobObject(Handle, JobObjectBasicProcessIdList, processIds, (uint)length, out returned)) snapshot.ProcessListError = Marshal.GetLastWin32Error();
                else {
                    snapshot.ProcessListQuerySucceeded = true;
                    snapshot.ProcessListReturnLength = returned;
                    snapshot.AssignedCount = unchecked((uint)Marshal.ReadInt32(processIds));
                    snapshot.ListedCount = unchecked((uint)Marshal.ReadInt32(processIds, 4));
                    snapshot.RequiredBytes = 8UL + (ulong)snapshot.AssignedCount * (ulong)IntPtr.Size;
                    if (snapshot.ListedCount <= capacity) {
                        snapshot.ProcessIds = new uint[(int)snapshot.ListedCount];
                        for (int i = 0; i < (int)snapshot.ListedCount; i++) {
                            long pid = Marshal.ReadIntPtr(processIds, 8 + i * IntPtr.Size).ToInt64();
                            if (pid <= 0 || pid > UInt32.MaxValue) { snapshot.ProcessListError = 87; snapshot.ProcessIds = new uint[0]; break; }
                            snapshot.ProcessIds[i] = (uint)pid;
                        }
                        uint minimumBytes = 8U + snapshot.ListedCount * (uint)IntPtr.Size;
                        snapshot.Complete = snapshot.ProcessListError == 0 && snapshot.AssignedCount == snapshot.ListedCount && returned >= minimumBytes && returned <= (uint)length;
                    }
                }
                snapshot.ProcessListCompletedTicks = Stopwatch.GetTimestamp();
            } finally { Marshal.FreeHGlobal(processIds); }
            return snapshot;
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
    public sealed class JobProcessSnapshot {
        public bool AccountingQuerySucceeded;
        public bool AccountingComplete;
        public int AccountingError;
        public uint AccountingActiveProcesses;
        public uint AccountingReturnLength;
        public uint AccountingRequiredBytes;
        public long AccountingStartedTicks;
        public long AccountingCompletedTicks;
        public bool ProcessListQuerySucceeded;
        public int ProcessListError;
        public uint AssignedCount;
        public uint ListedCount;
        public uint ProcessListReturnLength;
        public uint BufferCapacity;
        public uint BufferBytes;
        public ulong RequiredBytes;
        public long ProcessListStartedTicks;
        public long ProcessListCompletedTicks;
        public uint[] ProcessIds;
        public bool Complete;
    }
    public static JobLease CreateJob() { return new JobLease(); }
    public static void InitializeJob(JobLease job) { if (job == null) throw new ArgumentNullException("job"); job.Initialize(); }
    public static void CreateDirectoryNew(string path) { Need(CreateDirectoryW(path, IntPtr.Zero), "CreateDirectoryW"); }
    static void CloseLeaseHandle(ref IntPtr handle, string op) {
        if (handle == IntPtr.Zero) return;
        Need(CloseHandle(handle), op); handle = IntPtr.Zero;
    }
    public static uint LaunchConsoleMode(bool directServer, uint parentConsoleCodePage) {
        if (!directServer) return CREATE_NO_WINDOW;
        if (parentConsoleCodePage == 0) throw new InvalidOperationException("direct_server_parent_console_missing");
        return 0;
    }
    public static bool ConsoleContainsProcess(uint pid) {
        var pids = new uint[4096];
        uint count = GetConsoleProcessList(pids, (uint)pids.Length);
        if (count == 0) throw Last("GetConsoleProcessList");
        if (count > pids.Length) throw new InvalidOperationException("parent_console_process_list_incomplete");
        for (uint i = 0; i < count; i++) if (pids[i] == pid) return true;
        return false;
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
            lease.ParentConsoleCodePage = lease.DirectServer ? GetConsoleCP() : 0;
            uint consoleMode = LaunchConsoleMode(lease.DirectServer, lease.ParentConsoleCodePage);
            Need(CreateProcessW(app, mutableCommand, IntPtr.Zero, IntPtr.Zero, true, CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | consoleMode | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT, envText, cwd, ref si, out pi), "CreateProcessW");
            lease.RecordProcessCreated(pi.hProcess, pi.hThread, pi.dwProcessId);
            lease.RecordCreationFileTime(ProcessCreation(lease.Handle));
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
    public sealed class CandidateHandleLease { public uint Pid; public IntPtr Handle; public long Creation; public string Image; public string UserSid; public string Hash; public uint? ExitCode; }
    public sealed class CandidateInventoryCapture { public int Total; public int Inspected; public bool Truncated; public bool Complete; public string Error; public object[] Proofs; }
    static Dictionary<string, object> CandidateProof(uint pid, CandidateHandleLease lease) {
        return new Dictionary<string, object> { { "Pid", pid }, { "Lease", lease }, { "HandleRetained", false }, { "PidFromHandle", 0U }, { "VerifiedPidFromHandle", 0U }, { "CreationFileTime", 0L }, { "VerifiedCreationFileTime", 0L }, { "PidObserved", false }, { "VerifiedPidObserved", false }, { "CreationObserved", false }, { "VerifiedCreationObserved", false }, { "ImageObserved", false }, { "AccountObserved", false }, { "JobMembershipObservedBefore", false }, { "JobMembershipObservedAfter", false }, { "AliveObservedBefore", false }, { "AliveObservedAfter", false }, { "ExitCodeObserved", false }, { "IsProcessInJobBefore", false }, { "IsProcessInJobAfter", false }, { "AliveBefore", false }, { "AliveAfter", false }, { "Image", null }, { "UserSid", null }, { "ImageMatches", false }, { "HashMatches", false }, { "AccountMatches", false }, { "ListenerMatches", false }, { "ListenerCount", 0 }, { "ListenerRows", new object[0] }, { "ListenerRowsTruncated", false }, { "Errors", new List<string>() } };
    }
    static void CandidateError(List<string> errors, string operation, Exception error) {
        var win32 = error as Win32Exception;
        errors.Add(operation + ":" + error.GetType().Name + ":" + (win32 == null ? "not_win32" : win32.NativeErrorCode.ToString()));
    }
    static bool CandidateWithinDeadline(long deadlineTicks, List<string> errors) {
        if (Stopwatch.GetTimestamp() < deadlineTicks) return true;
        if (!errors.Contains("candidate_identity_deadline_expired")) errors.Add("candidate_identity_deadline_expired");
        return false;
    }
    static bool CandidateFactsObserved(object proof) {
        return FactBool(proof, "HandleRetained") && FactBool(proof, "PidObserved") && FactBool(proof, "VerifiedPidObserved") && FactBool(proof, "CreationObserved") && FactBool(proof, "VerifiedCreationObserved") && FactBool(proof, "ImageObserved") && FactBool(proof, "AccountObserved") && FactBool(proof, "JobMembershipObservedBefore") && FactBool(proof, "JobMembershipObservedAfter") && FactBool(proof, "AliveObservedBefore") && FactBool(proof, "AliveObservedAfter") && (FactBool(proof, "AliveBefore") && FactBool(proof, "AliveAfter") || FactBool(proof, "ExitCodeObserved"));
    }
    static object CaptureCandidateProof(JobLease job, IList registry, uint pid, long deadlineTicks) {
        var lease = new CandidateHandleLease { Pid = pid };
        registry.Add(lease);
        var proof = CandidateProof(pid, lease);
        var errors = (List<string>)proof["Errors"];
        if (!CandidateWithinDeadline(deadlineTicks, errors)) return proof;
        try { lease.Handle = OpenProcessHandle(pid); proof["HandleRetained"] = lease.Handle != IntPtr.Zero; }
        catch (Exception error) { CandidateError(errors, "OpenProcess", error); return proof; }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["PidFromHandle"] = HandleProcessId(lease.Handle); proof["PidObserved"] = true; } catch (Exception error) { CandidateError(errors, "GetProcessId", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["CreationFileTime"] = ProcessCreation(lease.Handle); proof["CreationObserved"] = true; lease.Creation = (long)proof["CreationFileTime"]; } catch (Exception error) { CandidateError(errors, "GetProcessTimes", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["Image"] = ProcessImage(lease.Handle); proof["ImageObserved"] = true; lease.Image = (string)proof["Image"]; } catch (Exception error) { CandidateError(errors, "QueryFullProcessImageNameW", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["UserSid"] = ProcessUserSid(lease.Handle); proof["AccountObserved"] = true; lease.UserSid = (string)proof["UserSid"]; } catch (Exception error) { CandidateError(errors, "ProcessUserSid", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["IsProcessInJobBefore"] = Contains(job, lease.Handle); proof["JobMembershipObservedBefore"] = true; } catch (Exception error) { CandidateError(errors, "IsProcessInJob(before)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["AliveBefore"] = HandleAlive(lease.Handle); proof["AliveObservedBefore"] = true; } catch (Exception error) { CandidateError(errors, "ProcessLiveness(before)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["VerifiedPidFromHandle"] = HandleProcessId(lease.Handle); proof["VerifiedPidObserved"] = true; } catch (Exception error) { CandidateError(errors, "GetProcessId(verify)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["VerifiedCreationFileTime"] = ProcessCreation(lease.Handle); proof["VerifiedCreationObserved"] = true; } catch (Exception error) { CandidateError(errors, "GetProcessTimes(verify)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["IsProcessInJobAfter"] = Contains(job, lease.Handle); proof["JobMembershipObservedAfter"] = true; } catch (Exception error) { CandidateError(errors, "IsProcessInJob(after)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["AliveAfter"] = HandleAlive(lease.Handle); proof["AliveObservedAfter"] = true; } catch (Exception error) { CandidateError(errors, "ProcessLiveness(after)", error); } }
        if ((FactBool(proof, "AliveObservedBefore") && !FactBool(proof, "AliveBefore")) || (FactBool(proof, "AliveObservedAfter") && !FactBool(proof, "AliveAfter"))) {
            if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["ExitCode"] = ExitCodeHandle(lease.Handle); proof["ExitCodeObserved"] = true; lease.ExitCode = (uint)proof["ExitCode"]; } catch (Exception error) { CandidateError(errors, "GetExitCodeProcess", error); } }
        }
        return proof;
    }
    public static CandidateInventoryCapture CaptureCandidateInventory(JobLease job, IList registry, uint[] pids, long deadlineTicks) {
        const int maximum = 32;
        var values = new List<object>(); var total = pids == null ? 0 : pids.Length; var limit = Math.Min(total, maximum); bool complete = total <= maximum;
        for (int i = 0; i < limit; i++) values.Add(CaptureCandidateProof(job, registry, pids[i], deadlineTicks));
        foreach (object value in values) if (((List<string>)((IDictionary)value)["Errors"]).Count != 0 || !CandidateFactsObserved((IDictionary)value)) complete = false;
        return new CandidateInventoryCapture { Total = total, Inspected = values.Count, Truncated = total > maximum, Complete = complete, Proofs = values.ToArray() };
    }
    public static void VerifyCandidateIdentity(JobLease job, IDictionary proof, long deadlineTicks) {
        var errors = Fact(proof, "Errors") as List<string>; if (errors == null) throw new InvalidOperationException("candidate_proof_errors_missing");
        var lease = Fact(proof, "Lease") as CandidateHandleLease; if (lease == null || lease.Handle == IntPtr.Zero) { errors.Add("candidate_verification_handle_missing"); return; }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["VerifiedPidFromHandle"] = HandleProcessId(lease.Handle); proof["VerifiedPidObserved"] = true; } catch (Exception error) { CandidateError(errors, "GetProcessId(verify_after_listener)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["VerifiedCreationFileTime"] = ProcessCreation(lease.Handle); proof["VerifiedCreationObserved"] = true; } catch (Exception error) { CandidateError(errors, "GetProcessTimes(verify_after_listener)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["IsProcessInJobAfter"] = Contains(job, lease.Handle); proof["JobMembershipObservedAfter"] = true; } catch (Exception error) { CandidateError(errors, "IsProcessInJob(after_listener)", error); } }
        if (CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["AliveAfter"] = HandleAlive(lease.Handle); proof["AliveObservedAfter"] = true; } catch (Exception error) { CandidateError(errors, "ProcessLiveness(after_listener)", error); } }
        if (FactBool(proof, "AliveObservedAfter") && !FactBool(proof, "AliveAfter") && CandidateWithinDeadline(deadlineTicks, errors)) { try { proof["ExitCode"] = ExitCodeHandle(lease.Handle); proof["ExitCodeObserved"] = true; lease.ExitCode = (uint)proof["ExitCode"]; } catch (Exception error) { CandidateError(errors, "GetExitCodeProcess(after_listener)", error); } }
    }
    static int CandidateErrorCount(object proof) { IEnumerable values = Fact(proof, "Errors") as IEnumerable; if (values == null) return 0; int count = 0; foreach (object ignored in values) count++; return count; }
    public sealed class CandidateInventoryDecision { public bool Valid; public string[] Reasons; public uint[] CandidatePids; public uint[] CapturedPids; }
    public static CandidateInventoryDecision ClassifyCandidateInventory(int total, bool truncated, bool complete, uint[] candidatePids, object[] proofs) {
        var reasons = new List<string>(); var expected = candidatePids ?? new uint[0]; var values = proofs ?? new object[0]; var captured = new List<uint>();
        if (total != expected.Length || total < 0) reasons.Add("candidate_inventory_total_mismatch");
        if (truncated || total > 32) reasons.Add("candidate_inventory_truncated");
        if (!complete) reasons.Add("candidate_inventory_incomplete");
        if (values.Length != Math.Min(Math.Max(total, 0), 32)) reasons.Add("candidate_inventory_identity_count_mismatch");
        for (int i = 0; i < values.Length; i++) {
            object proof = values[i]; uint pid = FactUInt(proof, "Pid"); captured.Add(pid);
            if (i >= expected.Length || pid == 0 || pid != expected[i]) reasons.Add("candidate_inventory_identity_order_or_pid_mismatch");
            if (CandidateErrorCount(proof) != 0) reasons.Add("candidate_inventory_api_error");
            if (!CandidateFactsObserved(proof)) reasons.Add("candidate_inventory_identity_incomplete");
            if (FactUInt(proof, "PidFromHandle") != pid || FactUInt(proof, "VerifiedPidFromHandle") != pid || FactLong(proof, "CreationFileTime") <= 0 || FactLong(proof, "CreationFileTime") != FactLong(proof, "VerifiedCreationFileTime")) reasons.Add("candidate_inventory_handle_identity_mismatch");
        }
        if (expected.Length == 0) reasons.Add("job_server_candidate_missing");
        else if (expected.Length != 1) reasons.Add("job_server_candidate_ambiguous");
        else if (values.Length == 1) {
            object proof = values[0];
            if (!FactBool(proof, "IsProcessInJobBefore") || !FactBool(proof, "IsProcessInJobAfter")) reasons.Add("job_server_candidate_job_membership_unconfirmed");
            if (!FactBool(proof, "AliveBefore") || !FactBool(proof, "AliveAfter")) reasons.Add("job_server_candidate_liveness_unconfirmed");
        }
        return new CandidateInventoryDecision { Valid = reasons.Count == 0, Reasons = UniqueReasons(reasons), CandidatePids = expected, CapturedPids = captured.ToArray() };
    }
    static Dictionary<string, object> CandidateControlFact(uint pid, bool apiError) {
        return new Dictionary<string, object> { { "Pid", pid }, { "HandleRetained", true }, { "PidFromHandle", pid }, { "VerifiedPidFromHandle", pid }, { "CreationFileTime", (long)pid + 1000 }, { "VerifiedCreationFileTime", (long)pid + 1000 }, { "PidObserved", true }, { "VerifiedPidObserved", true }, { "CreationObserved", true }, { "VerifiedCreationObserved", true }, { "ImageObserved", true }, { "AccountObserved", true }, { "JobMembershipObservedBefore", true }, { "JobMembershipObservedAfter", true }, { "AliveObservedBefore", true }, { "AliveObservedAfter", true }, { "ExitCodeObserved", false }, { "IsProcessInJobBefore", true }, { "IsProcessInJobAfter", true }, { "AliveBefore", true }, { "AliveAfter", true }, { "Errors", apiError ? new string[] { "injected_api_error" } : new string[0] } };
    }
    public static object[] CandidateInventoryContractControls() {
        var both = new object[] { CandidateControlFact(701, false), CandidateControlFact(702, false) };
        var two = ClassifyCandidateInventory(2, false, true, new uint[] { 701, 702 }, both);
        var errors = ClassifyCandidateInventory(1, false, false, new uint[] { 703 }, new object[] { CandidateControlFact(703, true) });
        var ids = new List<uint>(); var proofs = new List<object>(); for (uint pid = 704; pid < 736; pid++) { ids.Add(pid); proofs.Add(CandidateControlFact(pid, false)); } ids.Add(736);
        var truncated = ClassifyCandidateInventory(33, true, false, ids.ToArray(), proofs.ToArray());
        return new object[] {
            PureControl("two_candidate_identities_retained_but_refused", () => !two.Valid && ArrayEquals(two.CapturedPids, new uint[] { 701, 702 }) && Array.IndexOf(two.Reasons, "job_server_candidate_ambiguous") >= 0),
            PureControl("candidate_collection_api_error_refused", () => !errors.Valid && Array.IndexOf(errors.Reasons, "candidate_inventory_api_error") >= 0),
            PureControl("candidate_inventory_truncation_refused", () => !truncated.Valid && truncated.CapturedPids.Length == 32 && Array.IndexOf(truncated.Reasons, "candidate_inventory_truncated") >= 0)
        };
    }
    static StartupNoticeEvidence StartupControlEvidence(byte[] frame) {
        return new StartupNoticeEvidence { Connected = true, ReaderComplete = true, ClientPid = 701, ServerPid = 701, HandlePidBefore = 701, HandlePidAfter = 701, CapturedCreationFileTime = 1001, CreationFileTimeBefore = 1001, CreationFileTimeAfter = 1001, HandleRetained = true, JobMembershipBefore = true, JobMembershipAfter = true, AliveBefore = true, AliveAfter = true, Frame = DecodeStartupFrame(frame) };
    }
    public static object[] StartupNotificationContractControls() {
        byte[] okay = new byte[] { 0,0,0,26, 0,0,0,0, 14,0,0,0,0,0,0,0, 49,50,55,46,48,46,48,46,49,58,52,50,50,54 };
        byte[] otherEndpoint = new byte[] { 0,0,0,26, 0,0,0,0, 14,0,0,0,0,0,0,0, 49,50,55,46,48,46,48,46,49,58,52,50,50,55 };
        byte[] addrInUse = new byte[] { 0,0,0,4, 1,0,0,0 };
        byte[] timedOut = new byte[] { 0,0,0,4, 2,0,0,0 };
        byte[] error = new byte[] { 0,0,0,12, 3,0,0,0, 0,0,0,0,0,0,0,0 };
        byte[] unknown = new byte[] { 0,0,0,4, 4,0,0,0 };
        byte[] partial = new byte[] { 0,0,0,4, 1,0,0 };
        byte[] oversize = new byte[] { 0,0,16,1 };
        var trailing = new byte[okay.Length + 1]; Buffer.BlockCopy(okay, 0, trailing, 0, okay.Length);
        var results = new List<object>();
        results.Add(PureControl("direct_server_startup_reply_and_identity_positive", () => {
            var evidence = StartupControlEvidence(okay);
            return ClassifyDirectServerReadiness(evidence, new uint[] { 701 }, true, true, true).Valid;
        }));
        results.Add(PureControl("direct_server_inherits_existing_console", () => LaunchConsoleMode(true, 65001) == 0));
        results.Add(PureControl("finite_helper_keeps_no_window_mode", () => LaunchConsoleMode(false, 0) == CREATE_NO_WINDOW));
        results.Add(PureControl("direct_server_without_parent_console_refused", () => { try { LaunchConsoleMode(true, 0); return false; } catch (InvalidOperationException e) { return e.Message == "direct_server_parent_console_missing"; } }));
        results.Add(PureControl("direct_server_startup_wrong_endpoint_refused", () => !ClassifyDirectStartupNotice(StartupControlEvidence(otherEndpoint)).Valid));
        results.Add(PureControl("direct_server_startup_addr_in_use_refused", () => !ClassifyDirectStartupNotice(StartupControlEvidence(addrInUse)).Valid));
        results.Add(PureControl("direct_server_startup_timeout_refused", () => !ClassifyDirectStartupNotice(StartupControlEvidence(timedOut)).Valid));
        results.Add(PureControl("direct_server_startup_error_status_refused", () => !ClassifyDirectStartupNotice(StartupControlEvidence(error)).Valid));
        results.Add(PureControl("direct_server_startup_unknown_variant_refused", () => !ClassifyDirectStartupNotice(StartupControlEvidence(unknown)).Valid));
        results.Add(PureControl("direct_server_startup_partial_frame_refused", () => !ClassifyDirectStartupNotice(StartupControlEvidence(partial)).Valid));
        results.Add(PureControl("direct_server_startup_oversize_frame_refused", () => !ClassifyDirectStartupNotice(StartupControlEvidence(oversize)).Valid));
        results.Add(PureControl("direct_server_startup_trailing_frame_bytes_refused", () => !DecodeStartupFrame(trailing).Valid));
        results.Add(PureControl("direct_server_startup_reader_completion_unconfirmed", () => { var evidence = StartupControlEvidence(okay); evidence.ReaderComplete = false; return !ClassifyDirectStartupNotice(evidence).Valid; }));
        results.Add(PureControl("direct_server_startup_foreign_sender_refused", () => { var evidence = StartupControlEvidence(okay); evidence.ClientPid = 702; return !ClassifyDirectStartupNotice(evidence).Valid; }));
        results.Add(PureControl("direct_server_startup_creation_identity_refused", () => { var evidence = StartupControlEvidence(okay); evidence.CreationFileTimeAfter++; return !ClassifyDirectStartupNotice(evidence).Valid; }));
        results.Add(PureControl("direct_server_startup_job_membership_refused", () => { var evidence = StartupControlEvidence(okay); evidence.JobMembershipAfter = false; return !ClassifyDirectStartupNotice(evidence).Valid; }));
        results.Add(PureControl("direct_server_startup_early_exit_refused", () => { var evidence = StartupControlEvidence(okay); evidence.AliveAfter = false; return !ClassifyDirectStartupNotice(evidence).Valid; }));
        results.Add(PureControl("direct_server_extra_conhost_candidate_refused", () => !ClassifyDirectServerReadiness(StartupControlEvidence(okay), new uint[] { 701, 702 }, false, true, false).Valid));
        results.Add(PureControl("direct_server_listener_or_identity_proof_refused", () => !ClassifyDirectServerReadiness(StartupControlEvidence(okay), new uint[] { 701 }, true, true, false).Valid));
        results.Add(PureControl("direct_server_job_snapshot_api_error_refused", () => !ClassifyDirectServerReadiness(StartupControlEvidence(okay), new uint[] { 701 }, false, true, true).Valid));
        results.Add(PureControl("direct_server_candidate_inventory_incomplete_refused", () => !ClassifyDirectServerReadiness(StartupControlEvidence(okay), new uint[] { 701 }, true, false, true).Valid));
        return results.ToArray();
    }
    static Dictionary<string, object> DirectMemberSnapshot(uint[] pids) {
        uint count = (uint)pids.Length; ulong bytes = 8UL + (ulong)count * (ulong)IntPtr.Size;
        return new Dictionary<string, object> {
            { "AccountingQuerySucceeded", true }, { "AccountingComplete", true }, { "AccountingError", 0 },
            { "AccountingActiveProcesses", count }, { "AccountingReturnLength", 48 }, { "AccountingRequiredBytes", 48 },
            { "AccountingStartedTicks", 1 }, { "AccountingCompletedTicks", 2 }, { "ProcessListQuerySucceeded", true },
            { "ProcessListError", 0 }, { "AssignedCount", count }, { "ListedCount", count },
            { "ProcessListReturnLength", (uint)bytes }, { "BufferCapacity", 8192 }, { "BufferBytes", 65544 },
            { "RequiredBytes", bytes }, { "ProcessListStartedTicks", 3 }, { "ProcessListCompletedTicks", 4 },
            { "ProcessIds", pids }, { "Complete", true }
        };
    }
    static Dictionary<string,object> DirectMemberFact(uint pid, bool alive, bool identityValid) {
        return new Dictionary<string,object> { { "Created", true }, { "DirectServer", true }, { "Pid", pid }, { "HandlePid", pid }, { "CapturedCreationFileTime", (long)pid + 1000 }, { "CurrentCreationFileTime", (long)pid + 1000 }, { "Assigned", true }, { "HandleRetained", true }, { "IdentityValid", identityValid }, { "ProcessExitConfirmed", false }, { "CapturesComplete", false }, { "Alive", alive } };
    }
    public static object[] DirectServerMemberContractControls() {
        var direct = DirectMemberFact(701, true, true);
        var one = ClassifyJobMembers(DirectMemberSnapshot(new uint[] { 701 }), new object[] { direct });
        var two = ClassifyJobMembers(DirectMemberSnapshot(new uint[] { 701, 702 }), new object[] { direct });
        var unverified = ClassifyJobMembers(DirectMemberSnapshot(new uint[] { 701 }), new object[] { DirectMemberFact(701, false, false) });
        return new object[] {
            PureControl("direct_server_launch_lease_remains_live_candidate", () => one.Reasons.Length == 0 && ArrayEquals(one.CandidatePids, new uint[] { 701 }) && one.Members.Length == 1),
            PureControl("direct_server_and_conhost_both_remain_candidates", () => ArrayEquals(two.CandidatePids, new uint[] { 701, 702 }) && Array.IndexOf(ClassifyCandidateInventory(2, false, true, two.CandidatePids, new object[] { CandidateControlFact(701, false), CandidateControlFact(702, false) }).Reasons, "job_server_candidate_ambiguous") >= 0),
            PureControl("direct_server_unverified_lease_refused", () => unverified.Reasons.Length != 0 && unverified.CandidatePids.Length == 0)
        };
    }
    static bool ArrayEquals(uint[] left, uint[] right) { if (left == null || right == null || left.Length != right.Length) return false; for (int i = 0; i < left.Length; i++) if (left[i] != right[i]) return false; return true; }
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
    public static JobProcessSnapshot Snapshot(JobLease job) { return job.Snapshot(); }
    static object Fact(object source, string name) {
        IDictionary values = source as IDictionary;
        return values != null && values.Contains(name) ? values[name] : null;
    }
    static bool HasFact(object source, string name) { IDictionary values = source as IDictionary; return values != null && values.Contains(name); }
    static bool FactBool(object source, string name) { object value = Fact(source, name); try { return value != null && Convert.ToBoolean(value); } catch { return false; } }
    static int FactInt(object source, string name) { object value = Fact(source, name); try { return value == null ? 0 : Convert.ToInt32(value); } catch { return 0; } }
    static uint FactUInt(object source, string name) { object value = Fact(source, name); try { return value == null ? 0 : Convert.ToUInt32(value); } catch { return 0; } }
    static long FactLong(object source, string name) { object value = Fact(source, name); try { return value == null ? 0 : Convert.ToInt64(value); } catch { return 0; } }
    static string FactString(object source, string name) { object value = Fact(source, name); return value == null ? null : Convert.ToString(value); }
    static string[] UniqueReasons(List<string> reasons) {
        var unique = new List<string>(); var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (string reason in reasons) if (seen.Add(reason)) unique.Add(reason);
        return unique.ToArray();
    }
    static bool ValidExitedLaunch(object lease) {
        uint pid = FactUInt(lease, "Pid"); uint handlePid = FactUInt(lease, "HandlePid");
        long captured = FactLong(lease, "CapturedCreationFileTime"); long current = FactLong(lease, "CurrentCreationFileTime");
        return FactBool(lease, "Created") && FactBool(lease, "Assigned") && FactBool(lease, "HandleRetained") && FactBool(lease, "IdentityValid") && pid != 0 && pid == handlePid && captured > 0 && captured == current && FactBool(lease, "ProcessExitConfirmed") && FactBool(lease, "CapturesComplete");
    }
    public sealed class JobMembershipEvidence {
        public string[] Reasons;
        public object[] Members;
        public uint[] CandidatePids;
        public object[] OwnedLaunchFacts;
        public object[] ExcludedOwnedExits;
        public uint EligibleOwnedExits;
        public uint UnlistedOwnedExits;
        public ulong AccountingMinimum;
        public ulong AccountingMaximum;
    }
    public static JobMembershipEvidence ClassifyJobMembers(object snapshot, object[] leases) {
        var reasons = new List<string>(); var members = new List<object>(); var candidates = new List<uint>();
        var ownedFacts = new List<object>(); var excludedExits = new List<object>(); var pids = new List<uint>();
        string[] snapshotFields = { "AccountingQuerySucceeded", "AccountingComplete", "AccountingError", "AccountingActiveProcesses", "AccountingReturnLength", "AccountingRequiredBytes", "AccountingStartedTicks", "AccountingCompletedTicks", "ProcessListQuerySucceeded", "ProcessListError", "AssignedCount", "ListedCount", "ProcessListReturnLength", "BufferCapacity", "BufferBytes", "RequiredBytes", "ProcessListStartedTicks", "ProcessListCompletedTicks", "ProcessIds", "Complete" };
        foreach (string name in snapshotFields) if (!HasFact(snapshot, name)) reasons.Add("job_snapshot_fact_missing:" + name);
        object rawPids = Fact(snapshot, "ProcessIds"); IEnumerable pidValues = rawPids as IEnumerable;
        if (pidValues != null) foreach (object value in pidValues) { try { pids.Add(Convert.ToUInt32(value)); } catch { reasons.Add("job_process_list_pid_invalid"); } }
        bool accountingSucceeded = FactBool(snapshot, "AccountingQuerySucceeded");
        bool accountingComplete = FactBool(snapshot, "AccountingComplete");
        bool processListSucceeded = FactBool(snapshot, "ProcessListQuerySucceeded");
        bool listComplete = FactBool(snapshot, "Complete");
        uint active = FactUInt(snapshot, "AccountingActiveProcesses"); uint accountingLength = FactUInt(snapshot, "AccountingReturnLength");
        uint accountingRequired = FactUInt(snapshot, "AccountingRequiredBytes");
        long accountingStarted = FactLong(snapshot, "AccountingStartedTicks"); long accountingCompleted = FactLong(snapshot, "AccountingCompletedTicks");
        uint assigned = FactUInt(snapshot, "AssignedCount"); uint listed = FactUInt(snapshot, "ListedCount");
        uint processLength = FactUInt(snapshot, "ProcessListReturnLength"); uint capacity = FactUInt(snapshot, "BufferCapacity");
        uint bufferBytes = FactUInt(snapshot, "BufferBytes"); ulong requiredBytes = 0;
        long processListStarted = FactLong(snapshot, "ProcessListStartedTicks"); long processListCompleted = FactLong(snapshot, "ProcessListCompletedTicks");
        try { requiredBytes = Convert.ToUInt64(Fact(snapshot, "RequiredBytes")); } catch { reasons.Add("job_process_list_required_size_invalid"); }
        if (!accountingSucceeded) reasons.Add("job_accounting_query_failed:" + FactInt(snapshot, "AccountingError"));
        if (accountingSucceeded && (!accountingComplete || accountingRequired == 0 || accountingLength != accountingRequired)) reasons.Add("job_accounting_snapshot_incomplete");
        if (!processListSucceeded) reasons.Add("job_process_list_query_failed:" + FactInt(snapshot, "ProcessListError"));
        if (accountingStarted <= 0 || accountingCompleted < accountingStarted || processListStarted < accountingCompleted || processListCompleted < processListStarted) reasons.Add("job_snapshot_timestamps_incomplete");
        ulong expectedRequiredBytes = 8UL + (ulong)assigned * (ulong)IntPtr.Size;
        ulong minimumReturnedBytes = 8UL + (ulong)listed * (ulong)IntPtr.Size;
        if (!listComplete || assigned != listed || listed != (uint)pids.Count || capacity < assigned || requiredBytes != expectedRequiredBytes || requiredBytes > bufferBytes || processLength < minimumReturnedBytes || processLength > bufferBytes) reasons.Add("job_process_list_incomplete");
        var seen = new HashSet<uint>();
        foreach (uint pid in pids) if (pid == 0 || !seen.Add(pid)) reasons.Add("job_process_list_duplicate_or_invalid_pid");
        var createdLeases = new List<object>(); var validExitedPids = new HashSet<uint>(); uint directServerPid = 0; int directServerCount = 0;
        foreach (object lease in leases ?? new object[0]) {
            if (!FactBool(lease, "Created")) continue;
            createdLeases.Add(lease);
            uint pid = FactUInt(lease, "Pid"); uint handlePid = FactUInt(lease, "HandlePid");
            long captured = FactLong(lease, "CapturedCreationFileTime"); long current = FactLong(lease, "CurrentCreationFileTime");
            bool assignedLease = FactBool(lease, "Assigned"); bool handleRetained = FactBool(lease, "HandleRetained");
            bool identityValid = FactBool(lease, "IdentityValid") && pid != 0 && pid == handlePid && captured > 0 && captured == current;
            bool exited = FactBool(lease, "ProcessExitConfirmed"); bool captures = FactBool(lease, "CapturesComplete");
            bool directServer = FactBool(lease, "DirectServer"); bool alive = FactBool(lease, "Alive");
            bool listedInSnapshot = seen.Contains(pid) || seen.Contains(handlePid);
            bool validExit = !directServer && ValidExitedLaunch(lease);
            if (directServer) { directServerPid = pid; directServerCount++; }
            if (!assignedLease) reasons.Add("owned_launch_assignment_unconfirmed");
            if (!handleRetained || !identityValid) reasons.Add("owned_launch_identity_unconfirmed");
            if (directServer) {
                if (exited || !alive) reasons.Add("direct_server_live_lease_unconfirmed");
            } else {
                if (!exited) reasons.Add("owned_launch_exit_unconfirmed");
                if (!captures) reasons.Add("owned_launch_capture_unconfirmed");
            }
            if (validExit) validExitedPids.Add(pid);
            ownedFacts.Add(new { Pid = pid, HandlePid = handlePid, CapturedCreationFileTime = captured, CurrentCreationFileTime = current, Assigned = assignedLease, HandleRetained = handleRetained, IdentityValid = identityValid, IdentityError = FactString(lease, "IdentityError"), ProcessExitConfirmed = exited, ExitCode = Fact(lease, "ExitCode"), CapturesComplete = captures, ListedInProcessIdList = listedInSnapshot, DirectServer = directServer, Alive = alive, Classification = directServer && identityValid && assignedLease && alive && !exited ? "retained_direct_server_candidate" : validExit ? (listedInSnapshot ? "owned_exited_listed" : "owned_exited_unlisted") : "owned_exit_unconfirmed" });
            if (validExit) excludedExits.Add(new { Pid = pid, HandlePid = handlePid, CapturedCreationFileTime = captured, CurrentCreationFileTime = current, ExitCode = Fact(lease, "ExitCode"), CapturesComplete = captures, ListedInProcessIdList = listedInSnapshot });
        }
        if (directServerCount > 1) reasons.Add("direct_server_launch_identity_ambiguous");
        if (directServerCount == 1 && !seen.Contains(directServerPid)) reasons.Add("direct_server_missing_from_job_process_list");
        for (int i = 0; i < createdLeases.Count; i++) {
            uint pid = FactUInt(createdLeases[i], "Pid");
            for (int j = i + 1; j < createdLeases.Count; j++) if (pid != 0 && pid == FactUInt(createdLeases[j], "Pid")) reasons.Add("owned_launch_identity_ambiguous");
        }
        foreach (uint memberPid in pids) {
            var matches = new List<object>();
            foreach (object lease in createdLeases) if (FactUInt(lease, "Pid") == memberPid || FactUInt(lease, "HandlePid") == memberPid) matches.Add(lease);
            if (matches.Count > 1) { reasons.Add("owned_launch_identity_ambiguous"); members.Add(new { Pid = memberPid, Classification = "ambiguous_owned_launch" }); continue; }
            if (matches.Count == 0) { candidates.Add(memberPid); members.Add(new { Pid = memberPid, Classification = "candidate_pending_identity" }); continue; }
            object matched = matches[0]; uint launchPid = FactUInt(matched, "Pid"); uint handlePid = FactUInt(matched, "HandlePid");
            long captured = FactLong(matched, "CapturedCreationFileTime"); long current = FactLong(matched, "CurrentCreationFileTime");
            bool identityMatches = FactBool(matched, "Assigned") && FactBool(matched, "HandleRetained") && FactBool(matched, "IdentityValid") && launchPid == memberPid && handlePid == memberPid && captured > 0 && captured == current;
            bool exited = FactBool(matched, "ProcessExitConfirmed"); bool captures = FactBool(matched, "CapturesComplete");
            if (FactBool(matched, "DirectServer")) {
                bool live = FactBool(matched, "Alive");
                if (!identityMatches) reasons.Add("direct_server_member_identity_unconfirmed");
                if (!live || exited) reasons.Add("direct_server_member_not_live");
                if (identityMatches && live && !exited) candidates.Add(memberPid);
                members.Add(new { Pid = memberPid, Classification = identityMatches && live && !exited ? "direct_server_candidate" : "direct_server_identity_unconfirmed", OwnedLaunchPid = launchPid, HandlePid = handlePid, CapturedCreationFileTime = captured, CurrentCreationFileTime = current, Alive = live, ProcessExitConfirmed = exited, CapturesComplete = captures, IdentityError = FactString(matched, "IdentityError") });
                continue;
            }
            if (!identityMatches) reasons.Add("owned_launch_member_identity_mismatch");
            if (!exited) reasons.Add("owned_launch_member_exit_unconfirmed");
            if (!captures) reasons.Add("owned_launch_member_capture_unconfirmed");
            members.Add(new { Pid = memberPid, Classification = identityMatches && exited && captures ? "owned_exited" : "owned_exit_unconfirmed", OwnedLaunchPid = launchPid, HandlePid = handlePid, CapturedCreationFileTime = captured, CurrentCreationFileTime = current, ProcessExitConfirmed = exited, ExitCode = Fact(matched, "ExitCode"), CapturesComplete = captures, IdentityError = FactString(matched, "IdentityError") });
        }
        uint unlistedExits = 0;
        foreach (uint pid in validExitedPids) if (!seen.Contains(pid)) unlistedExits++;
        ulong accountingMinimum = listed; ulong accountingMaximum = accountingMinimum + unlistedExits;
        if (accountingSucceeded && accountingComplete && processListSucceeded && listComplete && assigned == listed && listed == (uint)pids.Count) {
            if ((ulong)active < accountingMinimum || (ulong)active > accountingMaximum) reasons.Add("job_accounting_process_list_relationship_inconsistent");
        }
        return new JobMembershipEvidence { Reasons = UniqueReasons(reasons), Members = members.ToArray(), CandidatePids = candidates.ToArray(), OwnedLaunchFacts = ownedFacts.ToArray(), ExcludedOwnedExits = excludedExits.ToArray(), EligibleOwnedExits = (uint)validExitedPids.Count, UnlistedOwnedExits = unlistedExits, AccountingMinimum = accountingMinimum, AccountingMaximum = accountingMaximum };
    }
    public static bool Contains(JobLease job, IntPtr process) { return job.Contains(process); }
    public static bool Terminate(JobLease job, uint code) { return job.Terminate(code); }
    public static bool CloseHandlePublic(IntPtr handle) { return handle != IntPtr.Zero && CloseHandle(handle); }
}
