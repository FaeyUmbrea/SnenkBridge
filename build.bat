@echo off
setlocal

cd /d "%~dp0"

cargo build -r --workspace

mkdir "%~dp0target\bundle\" 2>nul

copy /Y "%~dp0target\release\snenk_bridge.exe" "%~dp0target\bundle\snenk_bridge.exe"
copy /Y "%~dp0target\release\snenk_bridge_ui.exe" "%~dp0target\bundle\snenk_bridge_ui.exe"
copy /Y "%~dp0firewall.bat" "%~dp0target\bundle\firewall.bat"
copy /Y "%~dp0README.md" "%~dp0target\bundle\README.md"

echo Built Windows binaries in: %~dp0target\bundle\
