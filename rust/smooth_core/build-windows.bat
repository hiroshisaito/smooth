@echo off
rem Build smooth_core.lib for Windows (x86_64-pc-windows-msvc, static CRT).
rem Invoked by win.vcxproj PreBuildEvent. Idempotent.
setlocal
pushd "%~dp0"
cargo build --release --target x86_64-pc-windows-msvc
set BUILD_ERR=%ERRORLEVEL%
popd
endlocal & exit /b %BUILD_ERR%
