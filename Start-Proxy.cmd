@echo off
powershell.exe -NoProfile -File "%~dp0Start-SplitProxy.ps1" -SetSystemProxy
if errorlevel 1 pause
