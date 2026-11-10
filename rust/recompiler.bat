@echo off
rem Recompile tout bdauthor (coeur, rendu, interface) en mode release, en une seule commande (espace de travail Cargo).
rem   recompiler.bat           : compile les trois crates
rem   recompiler.bat tests     : compile puis lance les tests du coeur
rem   ajouter "nopause" pour ne pas attendre une touche a la fin (ex. : recompiler.bat tests nopause)
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
  echo cargo introuvable : installez Rust depuis https://rustup.rs puis rouvrez la fenetre.
  goto :echec
)

rem ffmpeg fourni a cote du projet (utilise par les tests)
for /d %%d in ("..\ffmpeg-*") do if exist "%%~fd\bin\ffmpeg.exe" set "BDAUTHOR_FFMPEG_DIR=%%~fd\bin"

set TESTS=0
set PAUSE=1
for %%a in (%*) do (
  if /i "%%a"=="tests" set TESTS=1
  if /i "%%a"=="nopause" set PAUSE=0
)

echo.
echo === Compilation : coeur, rendu, interface (espace de travail, un seul dossier target) ===
cargo build --release
if errorlevel 1 goto :echec
if "%TESTS%"=="1" (
  echo.
  echo === tests de bdauthor-core ===
  cargo test --release -p bdauthor-core
  if errorlevel 1 goto :echec
)

echo.
echo Compilation reussie : target\release\bdauthor.exe
if "%PAUSE%"=="1" pause
exit /b 0

:echec
echo.
echo *** ECHEC de la compilation : copiez les messages d'erreur ci-dessus. ***
if "%PAUSE%"=="1" pause
exit /b 1
