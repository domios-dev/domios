cask "domios" do
  arch arm: "aarch64", intel: "x64"

  version "@VERSION@"
  on_arm do
    sha256 "@SHA256_MACOS_ARM64@"
  end
  on_intel do
    sha256 "@SHA256_MACOS_X64@"
  end

  url "https://github.com/domios-dev/domios/releases/download/v#{version}/Domios_#{version}_#{arch}.dmg"
  name "Domios"
  desc "Desktop and server for AI agents"
  homepage "https://github.com/domios-dev/domios"

  depends_on macos: ">= :catalina"

  app "Domios.app"
  binary "#{appdir}/Domios.app/Contents/Resources/cli/domios"
  binary "#{appdir}/Domios.app/Contents/Resources/cli/codeterm"

  caveats do
    <<~EOS
      Launch Domios from Applications. The app includes its matching server.
      domios and codeterm select the CLI bundled inside Domios.app.
    EOS
  end
end
