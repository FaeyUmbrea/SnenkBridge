@echo off
setlocal

cd /d "%~dp0.."

cargo build -r -p snenk_bridge_ui

mkdir "%~dp0..\target\bundle\" 2>nul

copy /Y "%~dp0..\target\release\snenk_bridge_ui.exe" "%~dp0..\target\bundle\snenk_bridge_ui.exe"
copy /Y "%~dp0firewall.bat" "%~dp0..\target\bundle\firewall.bat"
copy /Y "%~dp0..\README.md" "%~dp0..\target\bundle\README.md"

echo Built Windows binaries in: %~dp0..\target\bundle\
