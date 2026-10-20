#define WIN32_LEAN_AND_MEAN
#include <windows.h>

static wchar_t *argument_end(wchar_t *argument) {
    if (*argument == L'"') {
        for (++argument; *argument && *argument != L'"'; ++argument) {}
        return *argument ? argument + 1 : argument;
    }
    while (*argument && *argument != L' ' && *argument != L'\t') ++argument;
    return argument;
}

static BOOL is_rustc(wchar_t *argument) {
    wchar_t *end = argument_end(argument);
    if (*argument == L'"') { ++argument; --end; }
    wchar_t *name = argument;
    for (wchar_t *at = argument; at < end; ++at) {
        if (*at == L'\\' || *at == L'/') name = at + 1;
    }
    const wchar_t *expected = L"rustc.exe";
    while (name < end && *expected && ((*name | 32) == *expected)) { ++name; ++expected; }
    return name == end && !*expected;
}

void mainCRTStartup(void) {
    wchar_t *command = argument_end(GetCommandLineW());
    while (*command == L' ' || *command == L'\t') ++command;
    wchar_t *wrapped = NULL;
    DWORD cache_length = GetEnvironmentVariableW(L"CODETERM_LANE_CACHE_WRAPPER", NULL, 0);
    if (cache_length && is_rustc(command)) {
        SIZE_T length = 0;
        while (command[length]) ++length;
        wrapped = HeapAlloc(GetProcessHeap(), 0, (cache_length + length + 4) * sizeof(wchar_t));
        if (!wrapped) ExitProcess(1);
        wrapped[0] = L'"';
        GetEnvironmentVariableW(L"CODETERM_LANE_CACHE_WRAPPER", wrapped + 1, cache_length);
        wrapped[cache_length] = L'"';
        wrapped[cache_length + 1] = L' ';
        lstrcpyW(wrapped + cache_length + 2, command);
        command = wrapped;
    }
    STARTUPINFOW startup = { sizeof(startup) };
    PROCESS_INFORMATION child = { 0 };
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    if (!*command || !CreateProcessW(NULL, command, NULL, NULL, TRUE, 0, NULL, NULL, &startup, &child)) ExitProcess(1);
    if (wrapped) HeapFree(GetProcessHeap(), 0, wrapped);
    CloseHandle(child.hThread);
    DWORD code = 1;
    if (WaitForSingleObject(child.hProcess, INFINITE) == WAIT_OBJECT_0) GetExitCodeProcess(child.hProcess, &code);
    CloseHandle(child.hProcess);
    ExitProcess(code);
}
