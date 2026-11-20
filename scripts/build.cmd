@echo off
rem Chay cargo voi moi truong MSVC + cmake + ninja (can cho crate ct2rs).
rem Can: VS Build Tools 2022, cmake va ninja trong PATH (vi du pip install cmake ninja).
rem Dung: scripts\build.cmd build --release   |   scripts\build.cmd test
call "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
if defined CT2_TOOLS set PATH=%CT2_TOOLS%;%PATH%
set PATH=%USERPROFILE%\.cargo\bin;%PATH%
cargo %*
