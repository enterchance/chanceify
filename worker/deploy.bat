@echo off
rem chanceify worker deploy. Double-click, or run: deploy.bat [embed|lastfm|both]
rem Needs Node.js. The first run opens a browser once to log in to Cloudflare.
setlocal
cd /d "%~dp0"
set WHICH=%1
if "%WHICH%"=="" set WHICH=both

where npx >nul 2>nul
if errorlevel 1 (
  echo Node.js is not installed. Get it from https://nodejs.org and run this again.
  pause
  exit /b 1
)

if /i "%WHICH%"=="lastfm" goto lastfm
echo.
echo === Song link page: chanceify-embed ===
call npx --yes wrangler deploy embed.js --name chanceify-embed --compatibility-date 2024-09-01
if errorlevel 1 goto failed
if /i "%WHICH%"=="embed" goto done

:lastfm
echo.
echo === Last.fm proxy: chanceify-lastfm ===
call npx --yes wrangler deploy lastfm-proxy.js --name chanceify-lastfm --compatibility-date 2024-09-01
if errorlevel 1 goto failed
echo.
set /p KEYS=Set or change the Last.fm key and secret now? (y/N) 
if /i "%KEYS%"=="y" (
  echo Paste the Last.fm API key when asked:
  call npx wrangler secret put LASTFM_KEY --name chanceify-lastfm
  echo Paste the Last.fm shared secret when asked:
  call npx wrangler secret put LASTFM_SECRET --name chanceify-lastfm
)

:done
echo.
echo Deployed. The addresses are printed above (https://...workers.dev).
pause
exit /b 0

:failed
echo.
echo Deploy failed. Read the message above.
pause
exit /b 1
