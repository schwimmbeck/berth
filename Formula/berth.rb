class Berth < Formula
  desc "Safe runtime and package manager for MCP servers"
  homepage "https://github.com/berth-dev/berth"
  license "Apache-2.0"
  head "https://github.com/berth-dev/berth.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/berth-cli")
  end

  test do
    assert_match "Berth CLI", shell_output("#{bin}/berth --help")
  end
end
