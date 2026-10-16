#define WIN32_LEAN_AND_MEAN
#include <windows.h>

void mainCRTStartup(void) {
    wchar_t *command = GetCommandLineW();
    if (*command == L'"') {
        for (++command; *command && *command != L'"'; ++command) {}
        if (*command) ++command;
    } else {
        while (*command && *command != L' ' && *command != L'\t') ++command;
    }
    while (*command == L' ' || *command == L'\t') ++command;
    STARTUPINFOW startup = { sizeof(startup) };
    PROCESS_INFORMATION child = { 0 };
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    if (!*command || !CreateProcessW(NULL, command, NULL, NULL, TRUE, 0, NULL, NULL, &startup, &child)) {
        ExitProcess(1);
    }
    CloseHandle(child.hThread);
    DWORD code = 1;
    if (WaitForSingleObject(child.hProcess, INFINITE) == WAIT_OBJECT_0) {
        GetExitCodeProcess(child.hProcess, &code);
    }
    CloseHandle(child.hProcess);
    ExitProcess(code);
}
