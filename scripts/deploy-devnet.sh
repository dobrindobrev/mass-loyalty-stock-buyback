#!/usr/bin/env bash
# Builds and deploys both programs to devnet, then initializes a demo setup.
# Uses its own deployer key (.keys/devnet-deployer.json) and never touches your global Solana config.
#   DEPLOYER_KEYPAIR=path  use another funded keypair
#   ARCH=v2                SBPF architecture for the build (default: v2, same as the tests)
#   RPC_URL=...            custom devnet RPC (default: public devnet)
set -euo pipefail
cd "$(dirname "$0")/.."

URL="${RPC_URL:-https://api.devnet.solana.com}"
DEPLOYER="${DEPLOYER_KEYPAIR:-.keys/devnet-deployer.json}"
ARCH="${ARCH:-v2}"
MIN_SOL=5

if [[ ! -f "$DEPLOYER" ]]; then
  mkdir -p "$(dirname "$DEPLOYER")"
  solana-keygen new --no-bip39-passphrase --silent -o "$DEPLOYER" >/dev/null
  chmod 600 "$DEPLOYER"
  echo "Created deployer key $DEPLOYER"
fi
ADDR=$(solana address -k "$DEPLOYER")
balance() { solana balance "$ADDR" --url "$URL" | awk '{print int($1)}'; }

echo "Deployer: $ADDR"
if (( $(balance) < MIN_SOL )); then
  echo "Requesting devnet airdrops..."
  for _ in 1 2 3; do
    solana airdrop 2 "$ADDR" --url "$URL" >/dev/null 2>&1 || true
    (( $(balance) >= MIN_SOL )) && break
  done
fi
if (( $(balance) < MIN_SOL )); then
  echo "Deployer has $(balance) SOL; need ~$MIN_SOL. The faucet is rate-limited."
  echo "Fund $ADDR at https://faucet.solana.com (devnet) and run this again."
  exit 1
fi

anchor keys sync >/dev/null
anchor build --arch "$ARCH"

for p in loyalty_buyback mock_swap; do
  echo "Deploying $p..."
  solana program deploy "target/deploy/$p.so" \
    --program-id "target/deploy/$p-keypair.json" \
    --keypair "$DEPLOYER" --fee-payer "$DEPLOYER" --url "$URL"
done

DEPLOYER_KEYPAIR="$DEPLOYER" RPC_URL="$URL" npx tsx scripts/init-devnet.ts
