// Caller: windows-dev-app.ps1; finite interactive DEV operation only.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

public static class DevDesktop {
    const int Deadline = 30000;
    const int MaximumText = 32768;
    static readonly string[] EnvironmentKeys = {
        "CODETERM_DEV", "CODETERM_DEV_INSTANCE", "CODETERM_DEV_API_PORT",
        "CODETERM_DEV_WORKSPACE_ROOT", "CODETERM_WEB_DIST", "CODETERM_CLI_PATH", "CODETERM_CDP_PORT",
        "CODETERM_VITE_PORT", "CODETERM_DEV_IDENTIFIER", "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "CLAUDE_CONFIG_DIR", "CODEX_HOME", "DOMIOS_CHAT_TRACE"
    };
    [DllImport("user32.dll")] static extern IntPtr GetShellWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetNamedPipeClientProcessId(SafePipeHandle pipe, out uint pid);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint pid);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool member);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int informationClass, IntPtr information, uint length, IntPtr returnedLength);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);

    static bool CurrentJobSurvivesLauncher() {
        bool member;
        if (!IsProcessInJob(Process.GetCurrentProcess().Handle, IntPtr.Zero, out member)) return false;
        if (!member) return true;
        int size = IntPtr.Size == 8 ? 144 : 112;
        IntPtr limits = Marshal.AllocHGlobal(size);
        try {
            if (!QueryInformationJobObject(IntPtr.Zero, 9, limits, (uint)size, IntPtr.Zero)) return false;
            const int KillOnJobClose = 0x2000;
            return (Marshal.ReadInt32(limits, 16) & KillOnJobClose) == 0;
        } finally { Marshal.FreeHGlobal(limits); }
    }

    static string Image(Process process) {
        return Path.GetFullPath(process.MainModule.FileName);
    }
    static Process PinPeer(uint pid, string expectedImage) {
        var process = Process.GetProcessById(checked((int)pid));
        try {
            var handle = process.Handle;
            if (process.HasExited || process.SessionId != Process.GetCurrentProcess().SessionId ||
                !String.Equals(Image(process), Path.GetFullPath(expectedImage), StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("DEV desktop process identity refused");
            IntPtr token;
            if (!OpenProcessToken(handle, 8, out token)) throw new InvalidOperationException("DEV desktop token unavailable");
            try {
                using (var identity = new WindowsIdentity(token)) {
                    if (!identity.User.Equals(WindowsIdentity.GetCurrent().User))
                        throw new InvalidOperationException("DEV desktop user identity refused");
                }
            } finally { CloseHandle(token); }
            return process;
        } catch { process.Dispose(); throw; }
    }
    public static void ValidateDesktopWindow(long window) {
        if (!Environment.UserInteractive || GetShellWindow() == IntPtr.Zero)
            throw new InvalidOperationException("DEV requires the current user's interactive Windows desktop");
        uint shellPid, automationPid;
        GetWindowThreadProcessId(GetShellWindow(), out shellPid);
        GetWindowThreadProcessId(new IntPtr(window), out automationPid);
        if (shellPid == 0 || shellPid != automationPid)
            throw new InvalidOperationException("DEV desktop automation owner refused");
        using (PinPeer(shellPid, Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), "explorer.exe"))) { }
    }
    static void WriteText(BinaryWriter writer, string value) {
        var bytes = Encoding.UTF8.GetBytes(value);
        if (bytes.Length > MaximumText) throw new InvalidOperationException("DEV launch input exceeds bound");
        writer.Write(bytes.Length); writer.Write(bytes);
    }
    static string ReadText(BinaryReader reader) {
        int length = reader.ReadInt32();
        if (length < 0 || length > MaximumText) throw new InvalidDataException("DEV launch input exceeds bound");
        var bytes = reader.ReadBytes(length);
        if (bytes.Length != length) throw new EndOfStreamException();
        return new UTF8Encoding(false, true).GetString(bytes);
    }
    static void WriteEnvironment(BinaryWriter writer) {
        writer.Write(EnvironmentKeys.Length);
        foreach (string key in EnvironmentKeys) {
            WriteText(writer, key); WriteText(writer, Environment.GetEnvironmentVariable(key) ?? "");
        }
    }
    static Dictionary<string, string> ReadEnvironment(BinaryReader reader) {
        int count = reader.ReadInt32();
        if (count != EnvironmentKeys.Length) throw new InvalidDataException("DEV environment refused");
        var environment = new Dictionary<string, string>();
        foreach (string expected in EnvironmentKeys) {
            if (ReadText(reader) != expected) throw new InvalidDataException("DEV environment key refused");
            environment.Add(expected, ReadText(reader));
        }
        if (environment["CODETERM_DEV"] != "1") throw new InvalidDataException("DEV intent refused");
        return environment;
    }
    static string ExactClient(string app) {
        var full = Path.GetFullPath(app);
        if (!Path.IsPathRooted(app) || !File.Exists(full) ||
            !String.Equals(Path.GetFileName(full), "domios-dev-app.exe", StringComparison.OrdinalIgnoreCase) ||
            !File.Exists(Path.Combine(Path.GetDirectoryName(full), "server-component.json")))
            throw new InvalidOperationException("DEV requires the staged native client and component inventory");
        return full;
    }
    public static int RunController(string script, string app, string powershell, Action<string, string, string> launch) {
        app = ExactClient(app);
        if (!String.IsNullOrEmpty(Environment.GetEnvironmentVariable("CODETERM_DEV_APP_ARGS")) &&
            Environment.GetEnvironmentVariable("CODETERM_DEV_APP_ARGS") != "[]")
            throw new InvalidOperationException("Interactive DEV accepts no arbitrary client arguments");
        if (Environment.GetEnvironmentVariable("CODETERM_DEV") != "1")
            throw new InvalidOperationException("Interactive DEV requires explicit development intent");
        var sid = WindowsIdentity.GetCurrent().User;
        var security = new PipeSecurity();
        security.SetAccessRuleProtection(true, false);
        security.AddAccessRule(new PipeAccessRule(sid, PipeAccessRights.FullControl, AccessControlType.Allow));
        string name = "domios-dev-desktop-" + Guid.NewGuid().ToString("N");
        using (var pipe = new NamedPipeServerStream(name, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous, 65536, 65536, security)) {
            string arguments = "-NoProfile -ExecutionPolicy Bypass -File " + DevAppProcess.Quote(script) +
                " -DesktopPipe " + DevAppProcess.Quote(name) + " -ControllerPid " + Process.GetCurrentProcess().Id;
            launch(powershell, arguments, Path.GetDirectoryName(script));
            var connection = pipe.BeginWaitForConnection(null, null);
            using (connection.AsyncWaitHandle) {
                if (!connection.AsyncWaitHandle.WaitOne(Deadline)) throw new TimeoutException("DEV desktop helper connection deadline exceeded");
                pipe.EndWaitForConnection(connection);
            }
            uint helperPid;
            if (!GetNamedPipeClientProcessId(pipe.SafePipeHandle, out helperPid))
                throw new InvalidOperationException("DEV desktop helper process unavailable");
            using (var helper = PinPeer(helperPid, powershell)) {
                var writer = new BinaryWriter(pipe, Encoding.UTF8, true);
                WriteText(writer, app); WriteText(writer, Path.GetDirectoryName(Path.GetDirectoryName(script)));
                WriteEnvironment(writer);
                writer.Flush();
                var reader = new BinaryReader(pipe, Encoding.UTF8, true);
                var output = Console.OpenStandardOutput();
                while (true) {
                    int kind = reader.ReadInt32();
                    if (kind == 3) {
                        if (ReadText(reader) != "helper_job_membership") throw new InvalidDataException("DEV desktop refusal frame invalid");
                        throw new InvalidOperationException("Interactive Windows shell retained DEV helper in a kill-on-close or unreadable job; no server launch attempted");
                    }
                    if (kind == 2) {
                        int code = reader.ReadInt32();
                        if (!helper.WaitForExit(Deadline)) throw new TimeoutException("DEV desktop helper exit deadline exceeded");
                        return code;
                    }
                    if (kind != 1) throw new InvalidDataException("DEV desktop output frame refused");
                    int length = reader.ReadInt32();
                    if (length < 0 || length > 65536) throw new InvalidDataException("DEV desktop output exceeds bound");
                    var bytes = reader.ReadBytes(length);
                    if (bytes.Length != length) throw new EndOfStreamException();
                    output.Write(bytes, 0, bytes.Length); output.Flush();
                }
            }
        }
    }
    sealed class Output : Stream {
        readonly BinaryWriter writer;
        readonly object gate = new object();
        public Output(Stream stream) { writer = new BinaryWriter(stream, Encoding.UTF8, true); }
        public void Complete(int code) { lock (gate) { writer.Write(2); writer.Write(code); writer.Flush(); } }
        public override void Write(byte[] bytes, int offset, int count) {
            if (count > 65536) throw new InvalidDataException("DEV output exceeds bound");
            lock (gate) { writer.Write(1); writer.Write(count); writer.Write(bytes, offset, count); writer.Flush(); }
        }
        public override void Flush() { }
        public override bool CanRead { get { return false; } }
        public override bool CanSeek { get { return false; } }
        public override bool CanWrite { get { return true; } }
        public override long Length { get { throw new NotSupportedException(); } }
        public override long Position { get { throw new NotSupportedException(); } set { throw new NotSupportedException(); } }
        public override int Read(byte[] b, int o, int c) { throw new NotSupportedException(); }
        public override long Seek(long o, SeekOrigin s) { throw new NotSupportedException(); }
        public override void SetLength(long n) { throw new NotSupportedException(); }
    }
    public static int RunHelper(string name, int controllerPid, string powershell, string script) {
        using (var pipe = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous)) {
            pipe.Connect(Deadline);
            uint peer;
            if (!GetNamedPipeServerProcessId(pipe.SafePipeHandle, out peer) || peer != controllerPid)
                throw new InvalidOperationException("DEV desktop controller identity refused");
            using (PinPeer(peer, powershell)) {
                if (!CurrentJobSurvivesLauncher()) {
                    var refusal = new BinaryWriter(pipe, Encoding.UTF8, true);
                    refusal.Write(3); WriteText(refusal, "helper_job_membership"); refusal.Flush();
                    return 1;
                }
                var reader = new BinaryReader(pipe, Encoding.UTF8, true);
                string app = ExactClient(ReadText(reader));
                string cwd = ReadText(reader);
                if (!String.Equals(cwd, Path.GetDirectoryName(Path.GetDirectoryName(script)), StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("DEV working directory refused");
                var environment = ReadEnvironment(reader);
                var output = new Output(pipe);
                using (var cancellation = new CancellationTokenSource()) {
                    var control = Task.Factory.StartNew(() => {
                        try { reader.ReadByte(); } catch (IOException) { } catch (ObjectDisposedException) { } finally { cancellation.Cancel(); }
                    }, TaskCreationOptions.LongRunning);
                    try {
                        int port;
                        if (!Int32.TryParse(environment["CODETERM_CDP_PORT"], out port) || port < 1024 || port > 65535 || port == 7685 ||
                            port.ToString() == environment["CODETERM_DEV_API_PORT"])
                            throw new InvalidDataException("DEV diagnostics port refused");
                        int code = DevAppProcess.RunOwned(app, new string[] { "--local-webview-diagnostics-port=" + port }, output, cwd, environment, cancellation.Token);
                        output.Complete(code);
                        return code;
                    } finally {
                        pipe.Dispose();
                        control.Wait();
                    }
                }
            }
        }
    }
}
