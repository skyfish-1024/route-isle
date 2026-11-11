@echo off
powershell.exe -NoProfile -File "%~dp0System-Proxy.ps1" -Action Restore
if errorlevel 1 pause
