# WSL2 から Windows ホスト側の Ollama に接続するための環境変数。
# 使い方: source scripts/wsl2-env.sh
# 常用する場合は ~/.bashrc に `source /path/to/scripts/wsl2-env.sh` を追記する。
#
# WSL2(NAT モード)では localhost は Windows ホストに届かないため、
# デフォルトゲートウェイ(= ホスト側 vEthernet の IP)を毎回導出する。
# ホスト IP は WSL 再起動で変わりうるので固定値は書かない。
export AGS_LLM_BASE_URL="http://$(ip route show default | awk '{print $3; exit}'):11434"

# Windows 側 Ollama にインストール済みのモデル(ollama list で確認・変更可)
export AGS_LLM_MODEL="gemma4:e2b-it-qat"
