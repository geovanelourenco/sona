; Instala o plugin "Sona" do OBS junto com o Sona Bridge, na pasta de plugins
; do OBS 28+ para todos os usuários (C:\ProgramData\obs-studio\plugins).
; Não é driver: é só um arquivo que o OBS carrega ao abrir.

!macro NSIS_HOOK_POSTINSTALL
  SetShellVarContext all
  CreateDirectory "$APPDATA\obs-studio\plugins\sona-obs\bin\64bit"
  CreateDirectory "$APPDATA\obs-studio\plugins\sona-obs\data"
  CopyFiles /SILENT "$INSTDIR\obs-plugin\sona-obs.dll" "$APPDATA\obs-studio\plugins\sona-obs\bin\64bit\sona-obs.dll"
  SetShellVarContext current
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  SetShellVarContext all
  RMDir /r "$APPDATA\obs-studio\plugins\sona-obs"
  SetShellVarContext current
!macroend
