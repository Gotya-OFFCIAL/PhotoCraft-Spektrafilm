// PhotoCraft-Spektrafilm launcher
//
// Win32 GUI-subsystem launcher: sets the product title environment variable,
// starts photocraft.exe from this executable's own directory, and exits.
// Built with VS2022 (cl /O2 /W4 /SUBSYSTEM:WINDOWS): no console window.
//
//   cl /nologo /O2 /W4 /Fe:"..\PhotoCraft-Spektrafilm.exe" launcher.c ^
//      /link /SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>

static const wchar_t kTitle[] = L"PhotoCraft-Spektrafilm";
static const wchar_t kChild[] = L"photocraft.exe";

static void fail(HWND owner, const wchar_t *what)
{
    wchar_t msg[512];
    _snwprintf_s(msg, 512, _TRUNCATE, L"PhotoCraft-Spektrafilm launcher\n%s", what);
    MessageBoxW(owner, msg, kTitle, MB_OK | MB_ICONERROR);
}

int main(void)
{
    // Rebrand the main window (photocraft reads PHOTOCRAFT_APP_TITLE) and switch
    // the in-app brand mark / window icon to the SpektraFilm tile.
    if (!SetEnvironmentVariableW(L"PHOTOCRAFT_APP_TITLE", kTitle)) {
        fail(NULL, L"SetEnvironmentVariableW(PHOTOCRAFT_APP_TITLE) failed.");
        return 1;
    }
    if (!SetEnvironmentVariableW(L"PHOTOCRAFT_BRAND_MARK", L"spektra")) {
        fail(NULL, L"SetEnvironmentVariableW(PHOTOCRAFT_BRAND_MARK) failed.");
        return 1;
    }

    // Resolve <own dir>\photocraft.exe.
    wchar_t self[MAX_PATH];
    DWORD n = GetModuleFileNameW(NULL, self, MAX_PATH);
    if (n == 0 || n >= MAX_PATH) {
        fail(NULL, L"GetModuleFileNameW failed.");
        return 1;
    }
    wchar_t *slash = wcsrchr(self, L'\\');
    if (!slash) {
        fail(NULL, L"Cannot determine install directory.");
        return 1;
    }
    *slash = L'\0';

    wchar_t exe[MAX_PATH], workdir[MAX_PATH];
    _snwprintf_s(exe, MAX_PATH, _TRUNCATE, L"%s\\%s", self, kChild);
    _snwprintf_s(workdir, MAX_PATH, _TRUNCATE, L"%s", self);
    if (GetFileAttributesW(exe) == INVALID_FILE_ATTRIBUTES) {
        fail(NULL, L"photocraft.exe not found next to the launcher.");
        return 1;
    }

    STARTUPINFOW si;
    ZeroMemory(&si, sizeof(si));
    si.cb = sizeof(si);
    PROCESS_INFORMATION pi;
    ZeroMemory(&pi, sizeof(pi));

    const wchar_t *raw_cmd = GetCommandLineW();
    const wchar_t *args = raw_cmd ? raw_cmd : L"";
    while (*args == L' ' || *args == L'\t') {
        ++args;
    }
    if (*args == L'"') {
        ++args;
        while (*args && *args != L'"') {
            ++args;
        }
        if (*args == L'"') {
            ++args;
        }
    } else {
        while (*args && *args != L' ' && *args != L'\t') {
            ++args;
        }
    }
    while (*args == L' ' || *args == L'\t') {
        ++args;
    }

    wchar_t cmd_line[4096];
    wchar_t *cmd_ptr = NULL;
    if (*args != L'\0') {
        _snwprintf_s(cmd_line, 4096, _TRUNCATE, L"\"%s\" %s", exe, args);
        cmd_ptr = cmd_line;
    }

    if (!CreateProcessW(exe, cmd_ptr, NULL, NULL, FALSE, 0, NULL, workdir, &si, &pi)) {
        wchar_t msg[560];
        _snwprintf_s(msg, 560, _TRUNCATE,
                     L"CreateProcessW(photocraft.exe) failed (win32 error %lu).",
                     GetLastError());
        fail(NULL, msg);
        return 1;
    }
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return 0;
}
