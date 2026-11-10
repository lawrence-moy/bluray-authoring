@echo off
rem Lance l'interface bdauthor (la compile d'abord si le programme n'existe pas encore).
setlocal
cd /d "%~dp0"
set "EXE=%~dp0target\release\bdauthor.exe"

if not exist "%EXE%" (
  echo Programme absent : compilation...
  call "%~dp0recompiler.bat" nopause
  if errorlevel 1 ( pause & exit /b 1 )
)

rem ffmpeg fourni a cote du projet
for /d %%d in ("..\ffmpeg-*") do if exist "%%~fd\bin\ffmpeg.exe" set "BDAUTHOR_FFMPEG_DIR=%%~fd\bin"

start "" "%EXE%"
