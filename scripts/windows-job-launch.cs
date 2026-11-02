using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

namespace JobLaunchProof {
    public sealed class NativeTree : IDisposable {
        [StructLayout(LayoutKind.Sequential)] struct Security { public int Size; public IntPtr Descriptor; public int Inherit; }
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct Startup {
            public int Size; public string Reserved, Desktop, Title;
            public int X, Y, Width, Height, Columns, Rows, Fill, Flags;
            public short Show, ReservedSize; public IntPtr ReservedData, Input, Output, Error;
        }
        [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr Process, Thread; public uint Pid, Tid; }
        [StructLayout(LayoutKind.Sequential)] struct BasicLimits {
            public long ProcessTime, JobTime; public uint Flags; public UIntPtr MinWorkingSet, MaxWorkingSet;
            public uint ActiveLimit; public UIntPtr Affinity; public uint Priority, Scheduling;
        }
        [StructLayout(LayoutKind.Sequential)] struct IoCounters { public ulong A, B, C, D, E, F; }
        [StructLayout(LayoutKind.Sequential)] struct Limits {
            public BasicLimits Basic; public IoCounters Io;
            public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
        }
        [StructLayout(LayoutKind.Sequential)] struct CreatedProcess {
            public IntPtr File, Process, Thread, Base;
            public uint DebugOffset, DebugSize;
            public IntPtr LocalBase, Start, ImageName; public ushort Unicode;
        }
        [StructLayout(LayoutKind.Explicit, Size = 176)] struct DebugEvent {
            [FieldOffset(0)] public uint Kind;
            [FieldOffset(4)] public uint Pid;
            [FieldOffset(8)] public uint Tid;
            [FieldOffset(16)] public CreatedProcess Created;
            [FieldOffset(16)] public uint Exception;
            [FieldOffset(16)] public uint ExitCode;
            [FieldOffset(16)] public IntPtr File;
        }
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool CreatePipe(out IntPtr read, out IntPtr write, ref Security security, uint size);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool CreateProcessW(string application, StringBuilder command, IntPtr processSecurity, IntPtr threadSecurity, bool inherit, uint flags, string environment, string cwd, ref Startup startup, out ProcessInfo process);
        [DllImport("kernel32.dll", SetLastError = true)] static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateJobObjectW(IntPtr security, IntPtr name);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int kind, ref Limits limits, int size);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int kind, out Limits limits, int size, IntPtr returned);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool WaitForDebugEvent(out DebugEvent result, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool ContinueDebugEvent(uint pid, uint tid, uint status);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool DuplicateHandle(IntPtr sourceProcess, IntPtr source, IntPtr destinationProcess, out IntPtr destination, uint access, bool inherit, uint options);
        [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetProcessTimes(IntPtr process, out long created, out long exited, out long kernel, out long user);
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool QueryFullProcessImageNameW(IntPtr process, uint flags, StringBuilder image, ref int length);
        [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr handle, out uint code);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint code);
        [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);

        readonly ConcurrentDictionary<uint, IntPtr> pins = new ConcurrentDictionary<uint, IntPtr>();
        readonly BlockingCollection<string> lines = new BlockingCollection<string>(64);
        readonly ManualResetEvent started = new ManualResetEvent(false);
        readonly object stderrLock = new object();
        readonly Stopwatch clock = Stopwatch.StartNew();
        readonly Thread debugger;
        string stderr = "";
        Exception failure;
        volatile bool stopping;
        bool disposed;
        IntPtr job;
        Task stdoutPump, stderrPump;
        StreamWriter input;
        public uint Pid { get; private set; }
        public uint JobFlags { get; private set; }
        public string[] ProcessEvidence { get { return evidence.ToArray(); } }
        public string[] ExitEvidence { get { return exits.ToArray(); } }
        readonly ConcurrentQueue<string> evidence = new ConcurrentQueue<string>();
        readonly ConcurrentQueue<string> exits = new ConcurrentQueue<string>();
        const uint DebugContinue = 0x10002, NotHandled = 0x80010001;
        readonly int boundMs;

        static void Require(bool ok, string operation) {
            if (!ok) throw new Win32Exception(Marshal.GetLastWin32Error(), operation);
        }
        public static string Quote(string value) {
            var result = new StringBuilder("\"");
            int slashes = 0;
            foreach (char c in value) {
                if (c == '\\') { slashes++; continue; }
                result.Append('\\', c == '"' ? slashes * 2 + 1 : slashes).Append(c);
                slashes = 0;
            }
            return result.Append('\\', slashes * 2).Append('"').ToString();
        }
        public NativeTree(string executable, string[] arguments, string environment, string cwd, long jobFlags, int budgetMs) {
            if (IntPtr.Size != 8) throw new PlatformNotSupportedException("Qualification requires native x64 Windows PowerShell");
            if (budgetMs < 1 || budgetMs > 90000) throw new ArgumentOutOfRangeException("budgetMs");
            boundMs = budgetMs;
            debugger = new Thread(() => Debug(executable, arguments, environment, cwd, jobFlags));
            debugger.IsBackground = true;
            debugger.Start();
            if (!started.WaitOne(10000)) { Dispose(); throw new TimeoutException("CreateProcess deadline"); }
            if (failure != null) { Dispose(); CheckFailure(); }
        }
        void CheckFailure() { if (failure != null) throw new InvalidOperationException("Native process observer failed", failure); }
        static FileStream PipeStream(IntPtr handle, FileAccess access) {
            return new FileStream(new SafeFileHandle(handle, true), access, 4096, false);
        }
        Task Pump(Stream stream, bool error) {
            return Task.Factory.StartNew(() => {
                using (var reader = new StreamReader(stream, Encoding.UTF8)) {
                    var buffer = new char[1024];
                    var pending = new StringBuilder();
                    int count;
                    while ((count = reader.Read(buffer, 0, buffer.Length)) > 0) {
                        if (error) {
                            lock (stderrLock) {
                                stderr += new string(buffer, 0, count);
                                if (stderr.Length > 65536) stderr = stderr.Substring(stderr.Length - 65536);
                            }
                        } else {
                            for (int i = 0; i < count; i++) {
                                if (buffer[i] == '\n') {
                                    lines.TryAdd(pending.ToString().TrimEnd('\r')); pending.Clear();
                                } else {
                                    if (pending.Length == 32768) throw new IOException("Oversized product stdout frame");
                                    pending.Append(buffer[i]);
                                }
                            }
                        }
                    }
                }
                if (!error) lines.CompleteAdding();
            }, TaskCreationOptions.LongRunning);
        }
        void Debug(string executable, string[] arguments, string environment, string cwd, long flags) {
            IntPtr inputRead = IntPtr.Zero, inputWrite = IntPtr.Zero, outputRead = IntPtr.Zero,
                outputWrite = IntPtr.Zero, errorRead = IntPtr.Zero, errorWrite = IntPtr.Zero;
            ProcessInfo process = new ProcessInfo();
            try {
                var security = new Security { Size = Marshal.SizeOf(typeof(Security)), Inherit = 1 };
                Require(CreatePipe(out inputRead, out inputWrite, ref security, 0), "Create stdin pipe");
                Require(CreatePipe(out outputRead, out outputWrite, ref security, 0), "Create stdout pipe");
                Require(CreatePipe(out errorRead, out errorWrite, ref security, 0), "Create stderr pipe");
                foreach (var handle in new[] { inputWrite, outputRead, errorRead })
                    Require(SetHandleInformation(handle, 1, 0), "Protect controller pipe");
                var startup = new Startup { Size = Marshal.SizeOf(typeof(Startup)), Flags = 0x100,
                    Input = inputRead, Output = outputWrite, Error = errorWrite, Show = 0 };
                var command = new StringBuilder(Quote(executable));
                foreach (var argument in arguments) command.Append(' ').Append(Quote(argument));
                // Debug events retain every descendant handle, including failed pre-READY breakaway children.
                Require(CreateProcessW(executable, command, IntPtr.Zero, IntPtr.Zero, true,
                    0x01000000 | 0x08000000 | 0x00000400 | 0x4 | 0x1, environment, cwd, ref startup, out process),
                    "Create suspended production launcher outside ambient job (breakaway must be granted)");
                Pid = process.Pid;
                bool contained;
                Require(IsProcessInJob(process.Process, IntPtr.Zero, out contained), "Read ambient membership");
                if (contained) throw new InvalidOperationException("Launcher retained an ancestor job; fixture refuses ambiguous containment");
                if (flags >= 0) {
                    job = CreateJobObjectW(IntPtr.Zero, IntPtr.Zero);
                    Require(job != IntPtr.Zero, "Create native job");
                    var limits = new Limits(); limits.Basic.Flags = (uint)flags;
                    Require(SetInformationJobObject(job, 9, ref limits, Marshal.SizeOf(typeof(Limits))), "Set native job limits");
                    Require(QueryInformationJobObject(job, 9, out limits, Marshal.SizeOf(typeof(Limits)), IntPtr.Zero), "Read native job limits");
                    JobFlags = limits.Basic.Flags;
                    if (JobFlags != (uint)flags) throw new InvalidOperationException("Job flags differ from requested flags");
                    Require(AssignProcessToJobObject(job, process.Process), "Assign suspended production launcher");
                }
                input = new StreamWriter(PipeStream(inputWrite, FileAccess.Write), new UTF8Encoding(false)) { AutoFlush = true };
                inputWrite = IntPtr.Zero;
                stdoutPump = Pump(PipeStream(outputRead, FileAccess.Read), false); outputRead = IntPtr.Zero;
                stderrPump = Pump(PipeStream(errorRead, FileAccess.Read), true); errorRead = IntPtr.Zero;
                CloseHandle(inputRead); inputRead = IntPtr.Zero;
                CloseHandle(outputWrite); outputWrite = IntPtr.Zero;
                CloseHandle(errorWrite); errorWrite = IntPtr.Zero;
                if (stopping) throw new OperationCanceledException("Fixture controller cancelled before resume");
                Require(ResumeThread(process.Thread) != UInt32.MaxValue, "Resume production launcher");
                int active = 0;
                do {
                    DebugEvent next;
                    int remaining = boundMs - (int)clock.ElapsedMilliseconds;
                    if (remaining <= 0) throw new TimeoutException("Native tree global deadline");
                    Require(WaitForDebugEvent(out next, (uint)remaining), "Wait for native process event");
                    uint status = DebugContinue;
                    try {
                        if (next.Kind == 3) {
                            IntPtr pin;
                            Require(DuplicateHandle(GetCurrentProcess(), next.Created.Process, GetCurrentProcess(), out pin, 0, false, 2), "Pin debug child");
                            if (!pins.TryAdd(next.Pid, pin)) { CloseHandle(pin); throw new IOException("Process ID reused within bounded fixture"); }
                            active++;
                            if (pins.Count > 128) throw new IOException("Native fixture process bound exceeded");
                            evidence.Enqueue(Identity(next.Pid));
                            if (stopping) Require(TerminateProcess(pin, 125), "Terminate late fixture descendant");
                            if (next.Created.File != IntPtr.Zero) CloseHandle(next.Created.File);
                        } else if (next.Kind == 5) { active--; exits.Enqueue(next.Pid + "\t" + next.ExitCode); }
                        else if (next.Kind == 6 && next.File != IntPtr.Zero) CloseHandle(next.File);
                        else if (next.Kind == 1 && next.Exception != 0x80000003) status = NotHandled;
                    } finally {
                        Require(ContinueDebugEvent(next.Pid, next.Tid, status), "Continue native process event");
                        started.Set();
                    }
                } while (active != 0);
            } catch (Exception error) {
                failure = error;
                foreach (var pin in pins.Values) TerminateProcess(pin, 125);
                if (process.Process != IntPtr.Zero) TerminateProcess(process.Process, 125);
            } finally {
                foreach (var handle in new[] { inputRead, inputWrite, outputRead, outputWrite, errorRead, errorWrite, process.Process, process.Thread })
                    if (handle != IntPtr.Zero) CloseHandle(handle);
                started.Set();
            }
        }
        IntPtr Pin(uint pid) {
            IntPtr handle;
            if (!pins.TryGetValue(pid, out handle)) throw new InvalidOperationException("PID has no owned process-event handle: " + pid);
            return handle;
        }
        public string Identity(uint pid) {
            var handle = Pin(pid);
            long created, exited, kernel, user;
            Require(GetProcessTimes(handle, out created, out exited, out kernel, out user), "Read kernel creation identity");
            var image = new StringBuilder(32768); int length = image.Capacity;
            Require(QueryFullProcessImageNameW(handle, 0, image, ref length), "Read pinned executable");
            return pid + "\t" + created + "\t" + image;
        }
        public bool InJob(uint pid) {
            bool result;
            Require(IsProcessInJob(Pin(pid), job, out result), "Read actual child job membership");
            return result;
        }
        public bool Alive(uint pid) { return WaitForSingleObject(Pin(pid), 0) == 258; }
        public void Send(string frame) { CheckFailure(); input.WriteLine(frame); }
        public void CloseInput() { if (input != null) { input.Dispose(); input = null; } }
        public string Line(int timeout) {
            string line;
            if (!lines.TryTake(out line, timeout)) { CheckFailure(); throw new IOException("Product emitted no bounded readiness frame: " + ErrorText()); }
            CheckFailure(); return line;
        }
        public uint Exit(uint pid, int timeout) {
            if (WaitForSingleObject(Pin(pid), (uint)timeout) != 0) throw new TimeoutException("Pinned process exit deadline: " + pid);
            uint code; Require(GetExitCodeProcess(Pin(pid), out code), "Read process exit"); return code;
        }
        public string ErrorText() { lock (stderrLock) return stderr; }
        public void Drain(int timeout) {
            if (!Task.WaitAll(new[] { stdoutPump, stderrPump }, timeout)) throw new TimeoutException("Product stdio EOF deadline");
        }
        public void Dispose() {
            if (disposed) return;
            stopping = true;
            Exception closeError = null;
            try { CloseInput(); } catch (IOException error) { closeError = error; }
            foreach (var pin in pins.Values)
                if (WaitForSingleObject(pin, 0) == 258 && !TerminateProcess(pin, 125))
                    Require(WaitForSingleObject(pin, 0) == 0, "Terminate exact fixture descendant");
            if (!debugger.Join(10000)) throw new TimeoutException("Native debugger cleanup deadline; retain fixture data");
            foreach (var pin in pins.Values) {
                if (WaitForSingleObject(pin, 1000) != 0) throw new TimeoutException("Fixture descendant still alive; retain fixture data");
            }
            var pumps = new List<Task>();
            if (stdoutPump != null) pumps.Add(stdoutPump);
            if (stderrPump != null) pumps.Add(stderrPump);
            if (!Task.WaitAll(pumps.ToArray(), 5000)) throw new TimeoutException("Fixture stdio cleanup deadline");
            foreach (var pin in pins.Values) CloseHandle(pin);
            if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
            started.Dispose();
            disposed = true;
            CheckFailure();
            if (closeError != null) throw new IOException("Fixture stdin close failed after process cleanup", closeError);
        }
    }
}
