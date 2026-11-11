@echo off
powershell.exe -NoProfile -STA -File "%~dp0Proxy-Gui.ps1" -Preview
if errorlevel 1 pause
