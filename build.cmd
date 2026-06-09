@echo off
setlocal

set "ROOT=%~dp0"
set "ROOT=%ROOT:~0,-1%"
set "ROOT_UNIX=%ROOT:\=/%"

if defined ProgramFiles (
  if exist "%ProgramFiles%\Git\bin\bash.exe" (
    set "GIT_BASH=%ProgramFiles%\Git\bin\bash.exe"
  ) else if exist "%ProgramFiles%\Git\usr\bin\bash.exe" (
    set "GIT_BASH=%ProgramFiles%\Git\usr\bin\bash.exe"
  )
)

if not defined GIT_BASH if defined ProgramFiles(x86) (
  if exist "%ProgramFiles(x86)%\Git\bin\bash.exe" (
    set "GIT_BASH=%ProgramFiles(x86)%\Git\bin\bash.exe"
  ) else if exist "%ProgramFiles(x86)%\Git\usr\bin\bash.exe" (
    set "GIT_BASH=%ProgramFiles(x86)%\Git\usr\bin\bash.exe"
  )
)

if not defined GIT_BASH (
  echo [build] Git Bash not found. Install Git for Windows or run build.ps1 directly.
  exit /b 1
)

"%GIT_BASH%" --noprofile --norc -lc "cd '%ROOT_UNIX%' && ./build.sh %*"
exit /b %ERRORLEVEL%
