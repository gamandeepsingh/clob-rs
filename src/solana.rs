use std::sync::Arc;

use sha2::{Digest, Sha256};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_program,
    transaction::Transaction,
};

use crate::models::{OrderType, Side};

pub struct SolanaClient {
    rpc: Arc<RpcClient>,
    authority: Keypair,
    program_id: Pubkey,
    pub market: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
}

impl SolanaClient {
    pub fn new(
        rpc_url: &str,
        keypair_path: &str,
        program_id: Pubkey,
        base_mint: Pubkey,
        quote_mint: Pubkey,
    ) -> anyhow::Result<Self> {
        let rpc = RpcClient::new_with_commitment(
            rpc_url.to_string(),
            CommitmentConfig::confirmed(),
        );

        let path = expand_tilde(keypair_path);
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("cannot read keypair at {path}: {e}"))?;
        let bytes: Vec<u8> = serde_json::from_str(&raw)?;
        let authority = Keypair::from_bytes(&bytes)?;

        let (market, _) = Pubkey::find_program_address(
            &[b"market", base_mint.as_ref(), quote_mint.as_ref()],
            &program_id,
        );

        Ok(Self { rpc: Arc::new(rpc), authority, program_id, market, base_mint, quote_mint })
    }

    pub async fn ensure_market_initialized(&self) -> anyhow::Result<()> {
        if self.rpc.get_account(&self.market).await.is_ok() {
            println!("[solana] market exists: {}", self.market);
            return Ok(());
        }
        println!("[solana] market not found — calling initialize_market...");
        self.initialize_market().await?;
        println!("[solana] market initialized: {}", self.market);
        Ok(())
    }

    async fn initialize_market(&self) -> anyhow::Result<()> {
        let data = anchor_discriminator("initialize_market").to_vec();
        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.authority.pubkey(), true),
                AccountMeta::new_readonly(self.base_mint, false),
                AccountMeta::new_readonly(self.quote_mint, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data,
        };
        self.send(&[ix]).await
    }

    /// Returns the on-chain order ID that was used as the PDA seed.
    /// Callers must store the mapping (in_memory_id → on_chain_id) to use settle_match later.
    pub async fn place_order(
        &self,
        side: Side,
        order_type: OrderType,
        price: u64,
        quantity: u64,
    ) -> anyhow::Result<u64> {
        // Read current order_count from chain — this is the seed the program will use.
        let on_chain_id = self.fetch_order_count().await?;

        let (order_pda, _) = Pubkey::find_program_address(
            &[b"order", self.market.as_ref(), &on_chain_id.to_le_bytes()],
            &self.program_id,
        );

        let mut data = anchor_discriminator("place_order").to_vec();
        data.push(side as u8);
        data.push(order_type as u8);
        data.extend_from_slice(&price.to_le_bytes());
        data.extend_from_slice(&quantity.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.market, false),
                AccountMeta::new(order_pda, false),
                AccountMeta::new(self.authority.pubkey(), true),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data,
        };

        self.send(&[ix]).await?;
        Ok(on_chain_id)
    }

    pub async fn settle_match(
        &self,
        buy_order_id: u64,
        sell_order_id: u64,
        fill_qty: u64,
        price: u64,
    ) -> anyhow::Result<()> {
        let (buy_pda, _) = Pubkey::find_program_address(
            &[b"order", self.market.as_ref(), &buy_order_id.to_le_bytes()],
            &self.program_id,
        );
        let (sell_pda, _) = Pubkey::find_program_address(
            &[b"order", self.market.as_ref(), &sell_order_id.to_le_bytes()],
            &self.program_id,
        );

        let auth = self.authority.pubkey();

        let mut data = anchor_discriminator("settle_match").to_vec();
        data.extend_from_slice(&fill_qty.to_le_bytes());
        data.extend_from_slice(&price.to_le_bytes());

        let ix = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(self.market, false),
                AccountMeta::new(buy_pda, false),
                AccountMeta::new(sell_pda, false),
                AccountMeta::new(auth, false),
                AccountMeta::new(auth, false),
                AccountMeta::new_readonly(auth, true),
            ],
            data,
        };

        self.send(&[ix]).await
    }

    async fn send(&self, instructions: &[Instruction]) -> anyhow::Result<()> {
        let blockhash = self.rpc.get_latest_blockhash().await?;
        let tx = Transaction::new_signed_with_payer(
            instructions,
            Some(&self.authority.pubkey()),
            &[&self.authority],
            blockhash,
        );
        self.rpc.send_and_confirm_transaction(&tx).await?;
        Ok(())
    }

    /// Reads market.order_count from the on-chain account data.
    /// Market layout (Anchor): [8 discriminator][32 authority][32 base_mint][32 quote_mint][8 order_count]
    async fn fetch_order_count(&self) -> anyhow::Result<u64> {
        let account = self.rpc.get_account(&self.market).await
            .map_err(|e| anyhow::anyhow!("failed to fetch market account: {e}"))?;

        const OFFSET: usize = 8 + 32 + 32 + 32; // = 104
        let data = &account.data;
        anyhow::ensure!(data.len() >= OFFSET + 8, "market account data too short");
        Ok(u64::from_le_bytes(data[OFFSET..OFFSET + 8].try_into()?))
    }
}

fn anchor_discriminator(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{name}"));
    h.finalize()[..8].try_into().unwrap()
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}
