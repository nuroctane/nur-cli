# Public incident corpus (DeFiHackLabs)

Index of **858 already-public** incidents from [SunWeb3Sec/DeFiHackLabs](https://github.com/SunWeb3Sec/DeFiHackLabs) (snapshot of their README TOC). This is a **lookup**, not an exploit cookbook.

Rules: lawful / in-scope only. Copy the **pattern** into a local Foundry test. Never broadcast. Never paste attacker contracts onto a funded host. For a named incident, open the DeFiHackLabs README anchor, then `reconstructing-public-postmortems`.

Live count may be higher than this snapshot. Re-pull the upstream README when hunting a 2026 incident.

## 2026 (115)

| Date | Incident |
|------|----------|
| 2026-08-23 | ArrakisGUNI - Uniswap V3 spot-price manipulation of vault mint/burn |
| 2026-08-23 | ArrakisGUNI - Uniswap V3 spot-price manipulation of vault mint/burn |
| 2026-08-22 | SandboxOFT - LayerZero delegate hijack via approveAndCall |
| 2026-08-20 | FlashstakeV2 - Mispriced reward pool, instant upfront reward extraction |
| 2026-08-19 | AllbridgeCCTP - Phantom CCTP deposit via unverified message attestation |
| 2026-08-15 | FoxLpBondsPool - Stale _stakeAmount from manipulable AMM spot quote |
| 2026-08-09 | USM - defund() price split-invariance rounding exploit |
| 2026-08-07 | Atomic - Flash-loan price oracle manipulation of lending collateral valuation |
| 2026-08-06 | PantherBase - Reality.eth governance timeout exploit (pre-production Base deployment, no user funds) |
| 2026-08-06 | PantherBase - Reality.eth governance timeout exploit (pre-production Base deployment, no user funds) |
| 2026-08-05 | StrongBlock - Governance takeover of abandoned Governor |
| 2026-08-03 | AIC - Pair skim / reserve-mismatch exploit (flash-swap leveraged) |
| 2026-08-02 | LpdFi (LOOPSDAO) - Spot-price manipulation + issue-boundary interest exploit |
| 2026-08-02 | LpdFi (LOOPSDAO) - Spot-price manipulation + issue-boundary interest exploit |
| 2026-07-30 | ExchangeIssuance (Index Coop) - TOCTOU positionMultiplier inflation via malicious pre-issue hook |
| 2026-07-30 | ExchangeIssuance (Index Coop) - TOCTOU positionMultiplier inflation via malicious pre-issue hook |
| 2026-07-26 | LULA - Reward recycle deflation manipulation via flash loan |
| 2026-07-25 | Projekt Reward Vault - Permissionless purchase-tracking self-dealing |
| 2026-07-25 | Projekt Reward Vault - Permissionless purchase-tracking self-dealing |
| 2026-07-24 | Lien Finance - Permissionless bond registration / payoff mispricing |
| 2026-07-19 | RWT Token - Deflationary burn-from-pair price manipulation |
| 2026-07-19 | RWT Token - Deflationary burn-from-pair price manipulation |
| 2026-07-16 | Perpetual Protocol - Access Control / Missing Permission Check |
| 2026-07-16 | Perpetual Protocol - Access Control / Missing Permission Check |
| 2026-07-16 | Perpetual Protocol - Access Control / Missing Permission Check |
| 2026-07-14 | Lumi Finance - ERC-4337 Validation-Phase Paymaster Approval |
| 2026-07-12 | Sodium - ERC-4337 Session-Key Validation Bypass |
| 2026-07-06 | SummerFi - FleetCommander NAV Inflation via Depegged xUSD |
| 2026-07-01 | edel-xstock - Price Oracle Manipulation |
| 2026-06-29 | Vault4626 - Business Logic Flaw |
| 2026-06-28 | AIDC - Business Logic Flaw |
| 2026-06-27 | CookFinanceIssuance - Price Oracle Manipulation |
| 2026-06-25 | OceanBPoolSideStaking - BPool single-sided join/exit math with SideStaking gulp accounting |
| 2026-06-25 | OceanBPoolSideStaking - BPool single-sided join/exit math with SideStaking gulp accounting |
| 2026-06-24 | DLMC - Reserve-derived livePrice manipulation |
| 2026-06-23 | RoyalRoyalties - Zero-amount ERC1155 batch transfer inflated Royal LDA tier balance |
| 2026-06-22 | ATM - LP Token Burn |
| 2026-06-22 | ATM - LP Token Burn |
| 2026-06-20 | OLPC - OLPC pair reserve manipulation |
| 2026-06-18 | JB - JB helper repeated cycle drains JB/USDT pair |
| 2026-06-17 | LBP - LBP balanceOf reward accounting |
| 2026-06-17 | LBP - LBP balanceOf reward accounting |
| 2026-06-17 | LBP - LBP balanceOf reward accounting |
| 2026-06-16 | DIP - Fee-on-Transfer Reserve Manipulation |
| 2026-06-15 | Thetanuts - Index vault component-share accounting flaw |
| 2026-06-14 | Aztec Connect - numRealTxs Proof/Settlement Mismatch (permissionless RollupProcessorV3) |
| 2026-06-09 | NovaBox - Constructor Dividend Checkpoint Bypass |
| 2026-06-09 | NovaBox - Constructor Dividend Checkpoint Bypass |
| 2026-06-07 | AmbientCrocSwapDex - Native surplus accounting flaw |
| 2026-06-06 | BOSS - BOSS helper mint/burn and transfer-tax pool skew |
| 2026-06-05 | AISOTHPresale - Fixed-price presale arbitrage |
| 2026-06-05 | AISOTHPresale - Fixed-price presale arbitrage |
| 2026-06-04 | ATM Token - Hidden transferFrom Auto-Swap Drain |
| 2026-06-04 | ATM Token - Hidden transferFrom Auto-Swap Drain |
| 2026-05-30 | AROS - Signature Replay |
| 2026-05-29 | YSDAO - Price Manipulation and Tax Bypass |
| 2026-05-28 | DxSale - Ownership Override Attack |
| 2026-05-28 | DxSale - Ownership Override Attack |
| 2026-05-27 | Joe Agent - Reentrancy in removeLiquidityViaContract |
| 2026-05-26 | SKP Token - Deliberately Engineered Drain (Insider Exploit / Rug Pull) |
| 2026-05-25 | WUSD.fi - _englove Sybil Incentive Abuse |
| 2026-05-25 | WUSD.fi - _englove Sybil Incentive Abuse |
| 2026-05-25 | WUSD.fi - _englove Sybil Incentive Abuse |
| 2026-05-22 | FractalProtocol - Business Logic Flaw |
| 2026-05-21 | MureDistribution - Signature Verification Bypass |
| 2026-05-20 | MAPProtocol - Arbitrary Mint |
| 2026-05-19 | ElevateFi - Reserve Price Manipulation |
| 2026-05-18 | TesseraSwap - Callback Repayment Price Spread |
| 2026-05-17 | SEAToken - Business Logic Flaw |
| 2026-05-17 | SEAToken - Business Logic Flaw |
| 2026-05-15 | AdsharesBridge - Insufficient Validation |
| 2026-05-12 | SQTokenStaking - Access Control |
| 2026-05-11 | HumaFinance - Credit Approval Bypass |
| 2026-05-11 | HumaFinance - Credit Approval Bypass |
| 2026-05-10 | Renegade - Uninitialized Proxy |
| 2026-05-07 | TrustedVolumes - Signature Replay |
| 2026-05-05 | Ekubo - Business Logic Flaw |
| 2026-05-01 | SharwaMarginTrading - Hegic collateral spot price manipulation |
| 2026-04-28 | JUDAO - JUDAO sell-hook reserve drain |
| 2026-04-28 | JUDAO - JUDAO sell-hook reserve drain |
| 2026-04-27 | Unverified_a152 - AllowanceTarget approval drain |
| 2026-04-25 | SingularityDynaVault - Oracle Misconfiguration / Share Inflation |
| 2026-04-23 | GiddyVaultV3 - Incomplete Signature Coverage |
| 2026-04-21 | KipseliPropAMM - Pricing / Decimals Mismatch |
| 2026-04-20 | ThetanutsVaultShareRounding - Vault Share Rounding Manipulation |
| 2026-04-20 | ThetanutsVaultShareRounding - Vault Share Rounding Manipulation |
| 2026-04-19 | AaveRebalancerCreditDelegation - Arbitrary External Call / Credit Delegation Abuse |
| 2026-04-15 | XLootStaking - Duplicate xLOOT Redemption |
| 2026-04-14 | Saturn Protocol - Vulnerability Disclosure |
| 2026-04-14 | Saturn Protocol - Vulnerability Disclosure |
| 2026-04-12 | SubQuerySettings - Settings access control |
| 2026-04-07 | SquidMulticallAllowanceDrain - Arbitrary Call / Wrong Approval |
| 2026-04-05 | PerpPair - Virtual AMM Manipulation |
| 2026-03-31 | WhalebitOracleManipulation - Algebra spot-price oracle manipulation |
| 2026-03-28 | VTSwapHook - Pricing Error in UniswapV4 Hook |
| 2026-03-27 | EST Token - Incorrect Token Burn Mechanism |
| 2026-03-24 | Univ3CollateralToken - Logic Error |
| 2026-03-24 | Univ3CollateralToken - Logic Error |
| 2026-03-23 | BCE - Deflationary Token Logic Error |
| 2026-03-19 | Revamp - Reward Accounting Drain |
| 2026-03-19 | Revamp - Reward Accounting Drain |
| 2026-03-16 | unverified - CheckoutPool Old BOC Missing Access Control |
| 2026-03-15 | StakeOnMe - Owner-privileged JAKE burn reserve drain |
| 2026-03-15 | StakeOnMe - Owner-privileged JAKE burn reserve drain |
| 2026-03-10 | AlkemiEarn - Business Logic |
| 2026-03-02 | Curve LlamaLend - Share price manipulation |
| 2026-02-22 | LAXO Token - Incorrect Burn Logic |
| 2026-02-16 | XDKRecycle - XDK recycle reserve manipulation |
| 2026-02-15 | Moonwell - Faulty Oracle |
| 2026-01-20 | Makina - Price Oracle Manipulation |
| 2026-01-20 | Makina - Price Oracle Manipulation |
| 2026-01-12 | MTToken - Incorrect Fee Logic |
| 2026-01-10 | FutureSwap - Unit Mismatch |
| 2026-01-09 | Truebit - OverFlow |
| 2026-01-01 | PRXVT - Bussiness Logic Flaw |

## 2025 (160)

| Date | Incident |
|------|----------|
| 2025-12-01 | yETH |
| 2025-11-10 | DRLVaultV3 |
| 2025-11-04 | Moonwell |
| 2025-11-03 | BalancerV2 |
| 2025-10-20 | SharwaFinance |
| 2025-10-07 | TokenHolder |
| 2025-10-04 | MIMSpell3 |
| 2025-09-18 | NGP |
| 2025-09-13 | Kame |
| 2025-08-31 | Hexotic |
| 2025-08-30 | EverValueCoin |
| 2025-08-27 | 0xf340 |
| 2025-08-23 | EquilibriaEPendle |
| 2025-08-23 | ABCCApp |
| 2025-08-22 | Unverified_6f7a |
| 2025-08-20 | MulticallWithXera |
| 2025-08-20 | 0x8d2e |
| 2025-08-19 | PresaleV5 |
| 2025-08-17 | AutoPooledTradingBot |
| 2025-08-16 | unverified |
| 2025-08-16 | d3xai |
| 2025-08-15 | SizeFlashLoanLooping |
| 2025-08-15 | PDZ |
| 2025-08-15 | SizeCredit |
| 2025-08-13 | BeefyZapRouter |
| 2025-08-13 | YuliAI |
| 2025-08-13 | coinbase |
| 2025-08-13 | Grizzifi |
| 2025-08-12 | BaseBebopSettlement |
| 2025-08-12 | Bebop |
| 2025-08-11 | WXC |
| 2025-08-08 | ArbitrumBaseSwapper |
| 2025-08-05 | MyCoinMaster |
| 2025-08-04 | BscInitcodeToken |
| 2025-08-01 | PendleReflector |
| 2025-07-29 | AnyswapWETHPermit |
| 2025-07-28 | SuperRare |
| 2025-07-28 | AvaxBIFKNPair |
| 2025-07-26 | Unverified670471 |
| 2025-07-26 | Unverified6883 |
| 2025-07-26 | MulticallWithETH |
| 2025-07-25 | WhereIsMyDragonTreasure |
| 2025-07-24 | SWAPPStaking |
| 2025-07-24 | EmptySetReserve |
| 2025-07-21 | BoJLeverageMarket |
| 2025-07-20 | Stepp2p |
| 2025-07-17 | unverified |
| 2025-07-17 | WETC |
| 2025-07-16 | StrategyLlamaLendConvex |
| 2025-07-16 | VDS |
| 2025-07-13 | UPENG |
| 2025-07-09 | GMX |
| 2025-07-05 | ActivePoolScrvUsd |
| 2025-07-05 | Unverified_54cd |
| 2025-07-05 | RANT |
| 2025-07-02 | FPC |
| 2025-07-02 | ActivePoolUrgentRedemption |
| 2025-06-29 | Stead |
| 2025-06-28 | InitcodeFactoryFees |
| 2025-06-26 | ResupplyFi |
| 2025-06-25 | Unverified_b5cb |
| 2025-06-25 | SiloFinance |
| 2025-06-25 | ParaSwapDAIApproval |
| 2025-06-23 | GradientMakerPool |
| 2025-06-20 | TokenVault |
| 2025-06-20 | HoldSafe |
| 2025-06-20 | Gangsterfinance |
| 2025-06-19 | SinstakeZombie |
| 2025-06-19 | BasePricePool |
| 2025-06-19 | BankrollStack |
| 2025-06-19 | BankrollNetwork |
| 2025-06-18 | BankrollStackPlus |
| 2025-06-17 | MetaPool |
| 2025-06-15 | WaleCoin |
| 2025-06-15 | FixedTokenBSwap |
| 2025-06-14 | TSAggregatorGeneric |
| 2025-06-12 | AAVEBoost |
| 2025-06-10 | unverified_8490 |
| 2025-06-09 | BitCrown |
| 2025-06-08 | TokenFactory |
| 2025-06-03 | PegaBall |
| 2025-05-31 | DogeAlliance |
| 2025-05-28 | Corkprotocol |
| 2025-05-28 | Unverified_91a1 |
| 2025-05-27 | UsualMoney |
| 2025-05-26 | YDT |
| 2025-05-25 | Unverified_0000 |
| 2025-05-25 | Dumbo |
| 2025-05-24 | RICE |
| 2025-05-20 | IRYSAI |
| 2025-05-19 | BetaPresale |
| 2025-05-18 | KRC |
| 2025-05-16 | Bitallx |
| 2025-05-14 | Unwarp |
| 2025-05-11 | MBUToken |
| 2025-05-09 | Nalakuvara_LotteryTicket50 |
| 2025-05-06 | Crosswise |
| 2025-04-29 | FlyLong |
| 2025-04-28 | tcdp |
| 2025-04-27 | bitdog |
| 2025-04-27 | multitransferswap |
| 2025-04-27 | AventaRewardClaim |
| 2025-04-26 | Lifeprotocol |
| 2025-04-26 | ImpermaxV3 |
| 2025-04-18 | BTNFT |
| 2025-04-16 | Roar |
| 2025-04-16 | YVToken |
| 2025-04-11 | Unverified 0x6077 |
| 2025-04-08 | Laundromat |
| 2025-04-07 | AmpKashi |
| 2025-04-04 | AIRWA |
| 2025-03-30 | LeverageSIR |
| 2025-03-28 | Alkimiya_IO |
| 2025-03-27 | YziAIToken |
| 2025-03-20 | BBXToken |
| 2025-03-18 | DCFToken |
| 2025-03-18 | unverified |
| 2025-03-16 | wKeyDAO |
| 2025-03-14 | H2O |
| 2025-03-11 | ZeroExSettler |
| 2025-03-11 | DUCKVADER |
| 2025-03-07 | StackMarket |
| 2025-03-07 | UNI |
| 2025-03-07 | SBRToken |
| 2025-03-06 | unverified |
| 2025-03-06 | RnsPay |
| 2025-03-06 | PTM |
| 2025-03-05 | 1inch Fusion V1 Settlement |
| 2025-03-04 | Pump |
| 2025-02-27 | Venus_ZKSync |
| 2025-02-26 | HenloKart |
| 2025-02-24 | INVISTECH |
| 2025-02-23 | HegicOptions |
| 2025-02-22 | Unverified_35bc |
| 2025-02-21 | StepHeroNFTs |
| 2025-02-21 | Bybit |
| 2025-02-15 | unverified_d4f1 |
| 2025-02-11 | Scorch |
| 2025-02-11 | LimitOrderProtocol |
| 2025-02-11 | FourMeme |
| 2025-02-08 | GMT7 |
| 2025-02-08 | Peapods Finance |
| 2025-02-01 | GoldReserve |
| 2025-01-28 | MCAI |
| 2025-01-26 | AIXBTForcedSwap |
| 2025-01-23 | ODOS |
| 2025-01-21 | Ast |
| 2025-01-18 | Paribus |
| 2025-01-14 | IdolsNFT |
| 2025-01-13 | Mosca2 |
| 2025-01-12 | Unilend |
| 2025-01-11 | RoulettePotV2 |
| 2025-01-10 | JPulsepot |
| 2025-01-08 | HORS |
| 2025-01-08 | LPMine |
| 2025-01-07 | IPC |
| 2025-01-06 | Mosca |
| 2025-01-04 | SorStaking |
| 2025-01-04 | 98#Token |
| 2025-01-01 | LAURAToken |

## 2024 (189)

| Date | Incident |
|------|----------|
| 2024-12-27 | Bizness |
| 2024-12-23 | Moonhacker |
| 2024-12-18 | Slurpy |
| 2024-12-16 | BTC24H |
| 2024-12-14 | JHY |
| 2024-12-10 | LABUBUToken |
| 2024-12-10 | CloberDEX |
| 2024-12-03 | Pledge |
| 2024-11-26 | NFTG |
| 2024-11-24 | Proxy_b7e1 |
| 2024-11-23 | Ak1111 |
| 2024-11-21 | Matez |
| 2024-11-20 | MainnetSettler |
| 2024-11-19 | PolterFinance |
| 2024-11-17 | MFT |
| 2024-11-14 | vETH |
| 2024-11-11 | DeltaPrime |
| 2024-11-09 | X319 |
| 2024-11-07 | ChiSale |
| 2024-11-07 | CoW |
| 2024-11-07 | UniV2 |
| 2024-11-05 | RPP |
| 2024-10-29 | BUBAI |
| 2024-10-26 | CompoundFork |
| 2024-10-22 | Erc20transfer |
| 2024-10-22 | VISTA |
| 2024-10-13 | MorphoBlue |
| 2024-10-11 | P719Token |
| 2024-10-06 | HYDT |
| 2024-10-06 | SASHAToken |
| 2024-10-05 | AIZPTToken |
| 2024-10-02 | LavaLending |
| 2024-10-01 | FireToken |
| 2024-09-26 | OnyxDAO |
| 2024-09-26 | Bedrock_DeFi |
| 2024-09-24 | MARA |
| 2024-09-23 | PestoToken |
| 2024-09-23 | Bankroll_Network |
| 2024-09-20 | DOGGO |
| 2024-09-20 | Shezmu |
| 2024-09-18 | Unverified_766a |
| 2024-09-15 | WXETA |
| 2024-09-13 | Unverified_5697 |
| 2024-09-13 | OTSeaStaking |
| 2024-09-12 | Unverified_03f9 |
| 2024-09-11 | INUMI |
| 2024-09-11 | INUMI_db27 |
| 2024-09-11 | AIRBTC |
| 2024-09-10 | Caterpillar_Coin_CUT |
| 2024-09-05 | Unverified_a89f |
| 2024-09-05 | PLN |
| 2024-09-05 | HANAToken |
| 2024-09-04 | Unverified_16d0 |
| 2024-09-03 | Penpiexyz_io |
| 2024-09-02 | Pythia |
| 2024-08-28 | Unverified_667d |
| 2024-08-28 | AAVE |
| 2024-08-20 | COCO |
| 2024-08-16 | Zenterest |
| 2024-08-16 | OMPxContract |
| 2024-08-14 | YodlRouter |
| 2024-08-13 | VOW |
| 2024-08-12 | iVest |
| 2024-08-06 | Novax |
| 2024-08-01 | Convergence |
| 2024-07-24 | Spectra_finance |
| 2024-07-23 | MEVbot_0xdd7c |
| 2024-07-16 | Lifiprotocol |
| 2024-07-14 | Minterest |
| 2024-07-12 | DoughFina |
| 2024-07-11 | SBT |
| 2024-07-11 | GAX |
| 2024-07-08 | LW |
| 2024-07-05 | DeFiPlaza |
| 2024-07-03 | UnverifiedContr_0x452E25 |
| 2024-07-02 | MRP |
| 2024-06-28 | Will |
| 2024-06-27 | APEMAGA |
| 2024-06-18 | INcufi |
| 2024-06-17 | Dyson_money |
| 2024-06-16 | WIFCOIN_ETH |
| 2024-06-11 | Crb2 |
| 2024-06-11 | JokInTheBox |
| 2024-06-10 | UwuLend - Price Manipulation |
| 2024-06-10 | Bazaar |
| 2024-06-08 | YYStoken |
| 2024-06-06 | SteamSwap |
| 2024-06-06 | MineSTM |
| 2024-06-04 | NCD |
| 2024-06-01 | VeloCore |
| 2024-05-31 | Liquiditytokens |
| 2024-05-31 | MixedSwapRouter |
| 2024-05-29 | SCROLL |
| 2024-05-29 | MetaDragon |
| 2024-05-28 | Tradeonorion |
| 2024-05-28 | EXcommunity |
| 2024-05-27 | RedKeysCoin |
| 2024-05-26 | NORMIE |
| 2024-05-22 | Burner |
| 2024-05-16 | TCH |
| 2024-05-14 | Sonne Finance |
| 2024-05-14 | PredyFinance |
| 2024-05-12 | TGC |
| 2024-05-10 | GFOX |
| 2024-05-10 | TSURU |
| 2024-05-08 | GPU |
| 2024-05-07 | SATURN |
| 2024-05-06 | OSN |
| 2024-04-30 | Yield |
| 2024-04-30 | PikeFinance |
| 2024-04-27 | BNBX |
| 2024-04-25 | NGFS |
| 2024-04-24 | XBridge |
| 2024-04-24 | YIEDL |
| 2024-04-22 | Z123 |
| 2024-04-20 | Rico |
| 2024-04-19 | HedgeyFinance |
| 2024-04-17 | UnverifiedContr_0x00C409 |
| 2024-04-16 | SATX |
| 2024-04-16 | MARS_DEFI |
| 2024-04-15 | GFA |
| 2024-04-15 | ChaingeFinance |
| 2024-04-14 | Hackathon |
| 2024-04-12 | FIL314 |
| 2024-04-12 | SumerMoney |
| 2024-04-12 | GROKD |
| 2024-04-10 | BigBangSwap |
| 2024-04-09 | UPS |
| 2024-04-08 | SQUID |
| 2024-04-04 | WSM |
| 2024-04-02 | HoppyFrogERC |
| 2024-04-01 | ATM |
| 2024-04-01 | OpenLeverage |
| 2024-03-29 | ETHFIN |
| 2024-03-29 | PrismaFi |
| 2024-03-28 | LavaLending |
| 2024-03-25 | ZongZi |
| 2024-03-24 | ARK |
| 2024-03-23 | CGT |
| 2024-03-21 | SSS |
| 2024-03-20 | Paraswap |
| 2024-03-14 | MO |
| 2024-03-13 | IT |
| 2024-03-12 | BBT |
| 2024-03-11 | Binemon |
| 2024-03-09 | Juice |
| 2024-03-09 | UnizenIO |
| 2024-03-07 | GHT |
| 2024-03-06 | ALP |
| 2024-03-06 | TGBS |
| 2024-03-05 | Woofi |
| 2024-02-28 | Seneca |
| 2024-02-28 | SMOOFSStaking |
| 2024-02-23 | Zoomer |
| 2024-02-23 | CompoundUni |
| 2024-02-23 | BlueberryProtocol |
| 2024-02-22 | SwarmMarkets |
| 2024-02-21 | DeezNutz404 |
| 2024-02-21 | GAIN |
| 2024-02-20 | EGGX |
| 2024-02-19 | RuggedArt |
| 2024-02-16 | ParticleTrade |
| 2024-02-15 | DualPools |
| 2024-02-15 | Babyloogn |
| 2024-02-15 | Miner |
| 2024-02-13 | MINER BSC |
| 2024-02-11 | Game |
| 2024-02-10 | FILX DN404 |
| 2024-02-08 | Pandora404 |
| 2024-02-05 | BurnsDefi |
| 2024-02-02 | ADC |
| 2024-02-01 | AffineDeFi |
| 2024-01-30 | XSIJ |
| 2024-01-30 | MIMSpell |
| 2024-01-29 | PeapodsFinance |
| 2024-01-28 | BarleyFinance |
| 2024-01-27 | CitadelFinance |
| 2024-01-25 | NBLGAME |
| 2024-01-22 | DAO_SoulMate |
| 2024-01-17 | BmiZapper |
| 2024-01-17 | SocketGateway |
| 2024-01-15 | Shell_MEV_0xa898 |
| 2024-01-12 | WiseLending |
| 2024-01-10 | Freedom |
| 2024-01-10 | LQDX Alert |
| 2024-01-04 | Gamma |
| 2024-01-02 | MIC |
| 2024-01-02 | RadiantCapital |
| 2024-01-01 | OrbitChain |

## 2023 (214)

| Date | Incident |
|------|----------|
| 2023-12-31 | Channels BUSD&USDC |
| 2023-12-30 | ChannelsFinance |
| 2023-12-28 | CCV |
| 2023-12-28 | DominoTT |
| 2023-12-25 | Telcoin |
| 2023-12-22 | PineProtocol |
| 2023-12-20 | TransitFinance |
| 2023-12-17 | Bob |
| 2023-12-17 | FloorProtocol |
| 2023-12-16 | GoodDollar |
| 2023-12-16 | KEST |
| 2023-12-16 | NFTTrader |
| 2023-12-14 | PHIL |
| 2023-12-13 | HYPR |
| 2023-12-11 | GoodCompound |
| 2023-12-09 | BCT |
| 2023-12-07 | HNet |
| 2023-12-06 | TIME |
| 2023-12-06 | ElephantStatus |
| 2023-12-05 | MAMO |
| 2023-12-05 | BEARNDAO |
| 2023-12-02 | bZxProtocol |
| 2023-12-01 | UnverifiedContr_0x431abb |
| 2023-11-30 | EEE |
| 2023-11-30 | CAROLProtocol |
| 2023-11-29 | Burntbubba |
| 2023-11-29 | AIS |
| 2023-11-28 | FiberRouter |
| 2023-11-25 | MetaLend |
| 2023-11-25 | TheNFTV2 |
| 2023-11-22 | KyberSwap |
| 2023-11-17 | Token8633_9419 |
| 2023-11-17 | ShibaToken |
| 2023-11-16 | WECO |
| 2023-11-15 | EHX |
| 2023-11-15 | XAI |
| 2023-11-15 | LinkDAO |
| 2023-11-14 | OKC Project |
| 2023-11-12 | MEV_0x8c2d |
| 2023-11-12 | MEV_0xa247 |
| 2023-11-11 | Mahalend |
| 2023-11-10 | Raft_fi |
| 2023-11-10 | GrokToken |
| 2023-11-07 | RBalancer |
| 2023-11-07 | MEVbot |
| 2023-11-06 | TrustPad |
| 2023-11-06 | TheStandard_io |
| 2023-11-06 | KR |
| 2023-11-02 | BRAND |
| 2023-11-02 | 3913Token |
| 2023-11-01 | SwampFinance |
| 2023-11-01 | OnyxProtocol |
| 2023-10-31 | UniBotRouter |
| 2023-10-30 | LaEeb |
| 2023-10-28 | AstridProtocol |
| 2023-10-24 | MaestroRouter2 |
| 2023-10-22 | OpenLeverage |
| 2023-10-19 | kTAF |
| 2023-10-18 | HopeLend |
| 2023-10-18 | MicDao |
| 2023-10-13 | BelugaDex |
| 2023-10-13 | WiseLending |
| 2023-10-12 | Platypus |
| 2023-10-11 | BH |
| 2023-10-08 | ZS |
| 2023-10-08 | pSeudoEth |
| 2023-10-07 | StarsArena |
| 2023-10-05 | DePayRouter |
| 2023-09-30 | FireBirdPair |
| 2023-09-29 | DEXRouter |
| 2023-09-26 | XSDWETHpool |
| 2023-09-24 | KubSplit |
| 2023-09-21 | CEXISWAP |
| 2023-09-16 | uniclyNFT |
| 2023-09-11 | 0x0DEX |
| 2023-09-09 | BFCToken |
| 2023-09-08 | APIG |
| 2023-09-07 | HCT |
| 2023-09-05 | QuantumWN |
| 2023-09-05 | JumpFarm |
| 2023-09-05 | HeavensGate |
| 2023-09-05 | FloorDAO |
| 2023-09-02 | DAppSocial |
| 2023-08-29 | EAC |
| 2023-08-27 | Balancer |
| 2023-08-26 | SVT |
| 2023-08-24 | GSS |
| 2023-08-21 | EHIVE |
| 2023-08-19 | BTC20 |
| 2023-08-18 | ExactlyProtocol |
| 2023-08-14 | ZunamiProtocol |
| 2023-08-09 | EarningFram |
| 2023-08-02 | CurveBurner |
| 2023-08-02 | Uwerx |
| 2023-08-01 | NeutraFinance |
| 2023-08-01 | LeetSwap |
| 2023-07-31 | GYMNET |
| 2023-07-30 | Curve |
| 2023-07-26 | Carson |
| 2023-07-24 | Palmswap |
| 2023-07-23 | MintoFinance |
| 2023-07-22 | ConicFinance02 |
| 2023-07-21 | ConicFinance |
| 2023-07-21 | SUT |
| 2023-07-20 | Utopia |
| 2023-07-20 | FFIST |
| 2023-07-18 | APEDAO |
| 2023-07-18 | BNO |
| 2023-07-17 | NewFi |
| 2023-07-15 | USDTStakingContract28 |
| 2023-07-12 | Platypus |
| 2023-07-12 | WGPT |
| 2023-07-11 | RodeoFinance |
| 2023-07-11 | Libertify |
| 2023-07-10 | ArcadiaFi |
| 2023-07-08 | CIVNFT |
| 2023-07-08 | Civfund |
| 2023-07-07 | LUSD |
| 2023-07-04 | BambooIA |
| 2023-07-04 | BaoCommunity |
| 2023-07-03 | AzukiDAO |
| 2023-06-30 | Biswap |
| 2023-06-30 | MyAi |
| 2023-06-28 | Themis |
| 2023-06-27 | UnverifiedContr_9ad32 |
| 2023-06-27 | STRAC |
| 2023-06-23 | SHIDO |
| 2023-06-21 | BabyDogeCoin02 |
| 2023-06-21 | BUNN |
| 2023-06-20 | MIM |
| 2023-06-19 | Contract_0x7657 |
| 2023-06-18 | ARA |
| 2023-06-17 | MidasCapitalXYZ |
| 2023-06-17 | Pawnfi |
| 2023-06-15 | CFC |
| 2023-06-15 | DEPUSDT_LEVUSDC |
| 2023-06-12 | Sturdy Finance |
| 2023-06-11 | SellToken04 |
| 2023-06-07 | CompounderFinance |
| 2023-06-06 | VINU |
| 2023-06-06 | UN |
| 2023-06-02 | NST SimpleSwap |
| 2023-06-01 | DDCoin |
| 2023-06-01 | Cellframenet |
| 2023-05-31 | ERC20TokenBank |
| 2023-05-29 | Jimbo |
| 2023-05-29 | BabyDogeCoin |
| 2023-05-29 | FAPEN |
| 2023-05-29 | NOON_NO |
| 2023-05-25 | GPT |
| 2023-05-24 | LocalTrade |
| 2023-05-24 | CS |
| 2023-05-23 | LFI |
| 2023-05-14 | landNFT |
| 2023-05-14 | SellToken03 |
| 2023-05-13 | Bitpaidio |
| 2023-05-13 | SellToken02 |
| 2023-05-12 | LW |
| 2023-05-11 | SellToken01 |
| 2023-05-10 | SNK |
| 2023-05-09 | MCC |
| 2023-05-09 | HODL |
| 2023-05-06 | Melo |
| 2023-05-05 | DEI |
| 2023-05-03 | NeverFall |
| 2023-05-02 | Level |
| 2023-04-28 | 0vix |
| 2023-04-27 | SiloFinance |
| 2023-04-24 | Axioma |
| 2023-04-19 | OLIFE |
| 2023-04-16 | Swapos V2 |
| 2023-04-15 | HundredFinance |
| 2023-04-13 | yearnFinance |
| 2023-04-12 | MetaPoint |
| 2023-04-11 | Paribus |
| 2023-04-09 | SushiSwap |
| 2023-04-05 | Sentiment |
| 2023-04-02 | Allbridge |
| 2023-03-28 | SafeMoon Hack |
| 2023-03-28 | THENA |
| 2023-03-25 | DBW |
| 2023-03-22 | BIGFI |
| 2023-03-17 | ParaSpace NFT |
| 2023-03-15 | Poolz |
| 2023-03-13 | EulerFinance |
| 2023-03-08 | DKP |
| 2023-03-07 | Phoenix |
| 2023-02-27 | LaunchZone |
| 2023-02-27 | SwapX |
| 2023-02-24 | EFVault |
| 2023-02-22 | DYNA |
| 2023-02-18 | RevertFinance |
| 2023-02-17 | Starlink |
| 2023-02-17 | Dexible |
| 2023-02-17 | Platypusdefi |
| 2023-02-10 | Sheep Token |
| 2023-02-10 | dForce |
| 2023-02-07 | CowSwap |
| 2023-02-06 | FDP Token |
| 2023-02-03 | Orion Protocol |
| 2023-02-03 | Spherax USDs |
| 2023-02-02 | BonqDAO |
| 2023-01-30 | BEVO |
| 2023-01-26 | TomInu Token |
| 2023-01-19 | SHOCO Token |
| 2023-01-19 | ThoreumFinance |
| 2023-01-18 | QTN Token |
| 2023-01-18 | UPS Token |
| 2023-01-17 | OmniEstate |
| 2023-01-16 | MidasCapital |
| 2023-01-11 | UFDao |
| 2023-01-11 | ROE |
| 2023-01-10 | BRA |
| 2023-01-03 | GDS |

## 2022 (129)

| Date | Incident |
|------|----------|
| 2022-12-30 | DFS |
| 2022-12-29 | JAY |
| 2022-12-25 | Rubic |
| 2022-12-23 | Defrost |
| 2022-12-14 | Nmbplatform |
| 2022-12-14 | FPR |
| 2022-12-13 | ElasticSwap |
| 2022-12-12 | BGLD |
| 2022-12-11 | Lodestar |
| 2022-12-11 | MEVbot_0x28d9 |
| 2022-12-10 | MUMUG |
| 2022-12-10 | TIFIToken |
| 2022-12-09 | NOVAToken |
| 2022-12-07 | AES |
| 2022-12-05 | RFB |
| 2022-12-05 | BBOX |
| 2022-12-02 | OverNight |
| 2022-12-01 | APC |
| 2022-11-29 | MBC & ZZSH |
| 2022-11-29 | SEAMAN |
| 2022-11-23 | NUM |
| 2022-11-22 | AUR |
| 2022-11-21 | SDAO |
| 2022-11-19 | AnnexFinance |
| 2022-11-18 | Polynomial |
| 2022-11-17 | UEarnPool |
| 2022-11-16 | SheepFarm |
| 2022-11-10 | DFXFinance |
| 2022-11-09 | brahTOPG |
| 2022-11-08 | MEV_0ad8 |
| 2022-11-08 | Kashi |
| 2022-11-07 | MooCAKECTX |
| 2022-11-05 | BDEX |
| 2022-10-27 | VTF |
| 2022-10-27 | Team Finance |
| 2022-10-26 | N00d Token |
| 2022-10-25 | ULME |
| 2022-10-24 | Market |
| 2022-10-24 | MulticallWithoutCheck |
| 2022-10-21 | OlympusDAO |
| 2022-10-20 | HEALTH Token |
| 2022-10-19 | BEGO Token |
| 2022-10-18 | HPAY |
| 2022-10-18 | PLTD Token |
| 2022-10-17 | Uerii Token |
| 2022-10-14 | INUKO Token |
| 2022-10-14 | EFLeverVault |
| 2022-10-14 | MEVBOT a47b |
| 2022-10-12 | ATK |
| 2022-10-11 | Rabby Wallet SwapRouter |
| 2022-10-11 | Templedao |
| 2022-10-10 | Carrot |
| 2022-10-09 | Xave Finance |
| 2022-10-06 | RES-Token |
| 2022-10-02 | Transit Swap |
| 2022-10-01 | BabySwap |
| 2022-10-01 | RL |
| 2022-10-01 | Thunder Brawl |
| 2022-09-29 | BXH |
| 2022-09-28 | MEVBOT Badc0de |
| 2022-09-23 | RADT-DAO |
| 2022-09-13 | MevBot Private TX |
| 2022-09-09 | DPC |
| 2022-09-08 | YYDS |
| 2022-09-08 | NewFreeDAO |
| 2022-09-08 | Ragnarok Online Invasion |
| 2022-09-06 | NXUSD |
| 2022-09-05 | ZoomproFinance |
| 2022-09-02 | ShadowFi |
| 2022-09-02 | Bad Guys by RPF |
| 2022-08-28 | DDC |
| 2022-08-24 | LuckyTiger NFT |
| 2022-08-16 | Circle_2 |
| 2022-08-13 | Circle |
| 2022-08-10 | XSTABLE Protocol |
| 2022-08-09 | ANCH |
| 2022-08-07 | EGD Finance |
| 2022-08-04 | EtnProduct |
| 2022-08-03 | Qixi |
| 2022-08-02 | Nomad Bridge |
| 2022-08-01 | Reaper Farm |
| 2022-07-25 | LPC |
| 2022-07-23 | Audius |
| 2022-07-13 | SpaceGodzilla |
| 2022-07-10 | Omni NFT |
| 2022-07-06 | FlippazOne NFT |
| 2022-07-01 | Quixotic - Optimism NFT Marketplace |
| 2022-06-26 | XCarnival |
| 2022-06-24 | Harmony's Horizon Bridge |
| 2022-06-18 | SNOOD |
| 2022-06-16 | InverseFinance |
| 2022-06-08 | GYMNetwork |
| 2022-06-08 | Optimism - Wintermute |
| 2022-06-06 | Discover |
| 2022-05-29 | NOVO Protocol |
| 2022-05-24 | HackDao |
| 2022-05-17 | ApeCoin |
| 2022-05-08 | Fortress Loans |
| 2022-04-30 | Saddle Finance |
| 2022-04-30 | Rari Capital/Fei Protocol |
| 2022-04-28 | DEUS DAO |
| 2022-04-24 | Wiener DOGE |
| 2022-04-23 | Akutar NFT |
| 2022-04-21 | Zeed Finance |
| 2022-04-16 | BeanstalkFarms |
| 2022-04-15 | Rikkei Finance |
| 2022-04-12 | ElephantMoney |
| 2022-04-11 | Creat Future |
| 2022-04-09 | GYMNetwork |
| 2022-03-29 | Ronin Network |
| 2022-03-29 | Redacted Cartel |
| 2022-03-27 | Revest Finance |
| 2022-03-26 | Auctus |
| 2022-03-22 | CompoundTUSDSweepTokenBypass |
| 2022-03-21 | OneRing Finance |
| 2022-03-20 | LI.FI |
| 2022-03-20 | Umbrella Network |
| 2022-03-15 | Agave Finance |
| 2022-03-15 | Hundred Finance |
| 2022-03-13 | Paraluni |
| 2022-03-09 | Fantasm Finance |
| 2022-03-05 | Bacon Protocol |
| 2022-03-03 | TreasureDAO |
| 2022-02-14 | BuildFinance - DAO |
| 2022-02-08 | Sandbox LAND |
| 2022-02-05 | Meter |
| 2022-02-04 | TecraSpace |
| 2022-01-28 | Qubit Finance |
| 2022-01-18 | Multichain (Anyswap) |

## 2021 (37)

| Date | Incident |
|------|----------|
| 2021-12-21 | Visor Finance |
| 2021-12-18 | Grim Finance |
| 2021-12-14 | Nerve Bridge |
| 2021-11-30 | MonoX Finance |
| 2021-11-23 | Ploutoz Finance |
| 2021-10-27 | Cream Finance |
| 2021-10-15 | Indexed Finance |
| 2021-09-16 | SushiSwap Miso |
| 2021-09-15 | Nimbus Platform |
| 2021-09-15 | NowSwap Platform |
| 2021-09-12 | ZABU Finance |
| 2021-09-03 | DAO Maker |
| 2021-08-30 | Cream Finance |
| 2021-08-17 | XSURGE |
| 2021-08-11 | Poly Network |
| 2021-08-04 | WaultFinance |
| 2021-08-04 | Popsicle |
| 2021-07-28 | Levyathan Finance |
| 2021-07-10 | Chainswap |
| 2021-07-02 | Chainswap |
| 2021-06-28 | SafeDollar |
| 2021-06-25 | xWin Finance |
| 2021-06-22 | Eleven Finance |
| 2021-06-07 | 88mph NFT |
| 2021-06-03 | PancakeHunny |
| 2021-05-27 | JulSwap |
| 2021-05-27 | BurgerSwap |
| 2021-05-19 | PancakeBunny |
| 2021-05-16 | bEarn |
| 2021-05-08 | Rari Capital |
| 2021-05-08 | Value Defi |
| 2021-05-02 | Spartan |
| 2021-04-28 | Uranium |
| 2021-03-08 | DODO |
| 2021-03-05 | Paid Network |
| 2021-02-04 | Yearn YDai |
| 2021-01-25 | Sushi Badger Digg |

## 2020 (9)

| Date | Incident |
|------|----------|
| 2020-12-29 | Cover Protocol |
| 2020-11-21 | Pickle Finance |
| 2020-10-26 | Harvest Finance |
| 2020-09-12 | bzx |
| 2020-08-04 | Opyn Protocol |
| 2020-06-28 | Balancer Protocol |
| 2020-06-18 | Bancor Protocol |
| 2020-04-19 | LendfMe |
| 2020-04-18 | UniSwapV1 |

## 2018 (3)

| Date | Incident |
|------|----------|
| 2018-10-07 | SpankChain |
| 2018-04-24 | SmartMesh |
| 2018-04-22 | Beauty Chain |

## 2017 (2)

| Date | Incident |
|------|----------|
| 2017-11-06 | Parity - 'Accidentally Killed It' |
| 2017-07-19 | Parity Multisig |

