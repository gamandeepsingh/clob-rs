use anchor_lang::prelude::*;

declare_id!("gm4yiTdk1Mv8menRGpRPqbXQTQNgKny1bP35Gaz8FCa");

#[program]
pub mod clob_program {
    use super::*;

    pub fn initialize_market(ctx: Context<InitializeMarket>) -> Result<()> {
        let market = &mut ctx.accounts.market;
        market.authority    = ctx.accounts.authority.key();
        market.base_mint    = ctx.accounts.base_mint.key();
        market.quote_mint   = ctx.accounts.quote_mint.key();
        market.order_count  = 0;
        market.total_volume = 0;
        market.bump         = ctx.bumps.market;
        Ok(())
    }

    pub fn place_order(
        ctx: Context<PlaceOrder>,
        side: Side,
        order_type: OrderType,
        price: u64,
        quantity: u64,
    ) -> Result<()> {
        let order_id   = ctx.accounts.market.order_count;
        let market_key = ctx.accounts.market.key();
        let trader_key = ctx.accounts.trader.key();

        ctx.accounts.market.order_count += 1;

        let order = &mut ctx.accounts.order;
        order.market     = market_key;
        order.trader     = trader_key;
        order.order_id   = order_id;
        order.side       = side;
        order.order_type = order_type;
        order.price      = price;
        order.quantity   = quantity;
        order.remaining  = quantity;
        order.timestamp  = Clock::get()?.unix_timestamp;
        order.bump       = ctx.bumps.order;

        Ok(())
    }

    pub fn cancel_order(_ctx: Context<CancelOrder>) -> Result<()> {
        Ok(())
    }

    pub fn settle_match(ctx: Context<SettleMatch>, fill_qty: u64, price: u64) -> Result<()> {
        require!(
            fill_qty <= ctx.accounts.buy_order.remaining
                && fill_qty <= ctx.accounts.sell_order.remaining,
            ClobError::InvalidFillQuantity
        );

        ctx.accounts.buy_order.remaining  -= fill_qty;
        ctx.accounts.sell_order.remaining -= fill_qty;
        ctx.accounts.market.total_volume  += fill_qty;

        emit!(TradeEvent {
            market:        ctx.accounts.market.key(),
            buy_order_id:  ctx.accounts.buy_order.order_id,
            sell_order_id: ctx.accounts.sell_order.order_id,
            price,
            quantity:      fill_qty,
            timestamp:     Clock::get()?.unix_timestamp,
        });

        if ctx.accounts.buy_order.remaining == 0 {
            ctx.accounts
                .buy_order
                .close(ctx.accounts.buy_trader.to_account_info())?;
        }

        if ctx.accounts.sell_order.remaining == 0 {
            ctx.accounts
                .sell_order
                .close(ctx.accounts.sell_trader.to_account_info())?;
        }

        Ok(())
    }
}

// ── Accounts ──────────────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct InitializeMarket<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + Market::INIT_SPACE,
        seeds = [b"market", base_mint.key().as_ref(), quote_mint.key().as_ref()],
        bump,
    )]
    pub market: Account<'info, Market>,

    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: used as seed only.
    pub base_mint: UncheckedAccount<'info>,

    /// CHECK: used as seed only.
    pub quote_mint: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PlaceOrder<'info> {
    #[account(
        mut,
        seeds = [b"market", market.base_mint.as_ref(), market.quote_mint.as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, Market>,

    #[account(
        init,
        payer = trader,
        space = 8 + Order::INIT_SPACE,
        seeds = [b"order", market.key().as_ref(), &market.order_count.to_le_bytes()],
        bump,
    )]
    pub order: Account<'info, Order>,

    #[account(mut)]
    pub trader: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelOrder<'info> {
    #[account(
        mut,
        seeds = [b"market", market.base_mint.as_ref(), market.quote_mint.as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, Market>,

    #[account(
        mut,
        close = trader,
        has_one = trader,
        has_one = market,
        seeds = [b"order", market.key().as_ref(), &order.order_id.to_le_bytes()],
        bump = order.bump,
    )]
    pub order: Account<'info, Order>,

    #[account(mut)]
    pub trader: Signer<'info>,
}

#[derive(Accounts)]
pub struct SettleMatch<'info> {
    #[account(
        mut,
        has_one = authority,
        seeds = [b"market", market.base_mint.as_ref(), market.quote_mint.as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, Market>,

    #[account(
        mut,
        has_one = market,
        constraint = buy_order.side == Side::Buy @ ClobError::InvalidOrderSide,
        seeds = [b"order", market.key().as_ref(), &buy_order.order_id.to_le_bytes()],
        bump = buy_order.bump,
    )]
    pub buy_order: Account<'info, Order>,

    #[account(
        mut,
        has_one = market,
        constraint = sell_order.side == Side::Sell @ ClobError::InvalidOrderSide,
        seeds = [b"order", market.key().as_ref(), &sell_order.order_id.to_le_bytes()],
        bump = sell_order.bump,
    )]
    pub sell_order: Account<'info, Order>,

    /// Rent returned here if buy_order is fully filled.
    #[account(mut, address = buy_order.trader)]
    pub buy_trader: SystemAccount<'info>,

    /// Rent returned here if sell_order is fully filled.
    #[account(mut, address = sell_order.trader)]
    pub sell_trader: SystemAccount<'info>,

    pub authority: Signer<'info>,
}

// ── State ─────────────────────────────────────────────────────────────────────

#[account]
#[derive(InitSpace)]
pub struct Market {
    pub authority:    Pubkey,
    pub base_mint:    Pubkey,
    pub quote_mint:   Pubkey,
    pub order_count:  u64,
    pub total_volume: u64,
    pub bump:         u8,
}

#[account]
#[derive(InitSpace)]
pub struct Order {
    pub market:     Pubkey,
    pub trader:     Pubkey,
    pub order_id:   u64,
    pub side:       Side,
    pub order_type: OrderType,
    pub price:      u64,
    pub quantity:   u64,
    pub remaining:  u64,
    pub timestamp:  i64,
    pub bump:       u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum OrderType {
    Limit,
    Market,
}

// ── Events ────────────────────────────────────────────────────────────────────

#[event]
pub struct TradeEvent {
    pub market:        Pubkey,
    pub buy_order_id:  u64,
    pub sell_order_id: u64,
    pub price:         u64,
    pub quantity:      u64,
    pub timestamp:     i64,
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[error_code]
pub enum ClobError {
    #[msg("order side does not match expected side")]
    InvalidOrderSide,
    #[msg("fill quantity exceeds order remaining")]
    InvalidFillQuantity,
}
