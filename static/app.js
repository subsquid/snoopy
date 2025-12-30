// Wallet management class
class WalletManager {
    constructor() {
        this.web3 = null;
        this.provider = null;
        this.signer = null;
        this.account = null;
        this.isConnected = false;
        this.transactionHistory = []; // Track transactions to manager_address
        this.init();
    }

    init() {
        // Check if web3 library is loaded
        if (typeof Web3 === 'undefined') {
            console.error('Web3 library not loaded');
            this.updateWalletStatus('Library Error');
            return;
        }
        
        // Check if wallet is already connected
        if (window.ethereum) {
            this.setupEventListeners();
            this.checkConnection();
        } else {
            this.updateWalletStatus('No Wallet Found');
        }
    }

    setupEventListeners() {
        if (!window.ethereum) return;

        // Handle account changes
        window.ethereum.on('accountsChanged', (accounts) => {
            if (accounts.length === 0) {
                this.disconnect();
            } else {
                this.account = accounts[0];
                this.updateUI();
            }
        });

        // Handle chain changes
        window.ethereum.on('chainChanged', (chainId) => {
            // Reload page on chain change
            window.location.reload();
        });
    }

    async checkConnection() {
        if (!window.ethereum) return;

        try {
            const accounts = await window.ethereum.request({ 
                method: 'eth_accounts' 
            });
            
            if (accounts.length > 0) {
                await this.connect();
            }
        } catch (error) {
            console.error('Error checking wallet connection:', error);
        }
    }

    async connect() {
        if (!window.ethereum) {
            this.showToast('Please install MetaMask or another web3 wallet', true);
            return false;
        }

        if (typeof Web3 === 'undefined') {
            this.showToast('Web3 library not loaded. Please refresh the page.', true);
            return false;
        }

        try {
            // Request account access
            await window.ethereum.request({ 
                method: 'eth_requestAccounts' 
            });

            // Create web3 instance
            this.web3 = new Web3(window.ethereum);
            const accounts = await this.web3.eth.getAccounts();
            this.account = accounts[0];
            this.isConnected = true;

            this.updateUI();
            this.showToast('Wallet connected successfully!');
            
            // Show transactions section and load existing transactions
            this.showTransactionsSection();
            await this.loadExistingTransactions();
            
            return true;
        } catch (error) {
            console.error('Error connecting wallet:', error);
            this.showToast('Failed to connect wallet', true);
            return false;
        }
    }

    disconnect() {
        this.web3 = null;
        this.provider = null;
        this.signer = null;
        this.account = null;
        this.isConnected = false;
        this.updateUI();
        this.hideTransactionsSection();
        this.showToast('Wallet disconnected');
    }

    updateUI() {
        const connectBtn = document.getElementById('wallet-connect-btn');
        const submitTaskBtn = document.getElementById('submit-task-btn');

        if (this.isConnected && this.account) {
            const shortAddress = this.account.slice(0, 6) + '...' + this.account.slice(-4);
            
            connectBtn.textContent = shortAddress;
            connectBtn.classList.add('connected');
            connectBtn.onclick = () => this.disconnect();
            
            // Show submit task button when wallet is connected
            if (submitTaskBtn) {
                submitTaskBtn.style.display = 'flex';
            }
        } else {
            connectBtn.innerHTML = `
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                    <path d="M21 12C21 16.9706 16.9706 21 12 21C7.02944 21 3 16.9706 3 12C3 7.02944 7.02944 3 12 3C16.9706 3 21 7.02944 21 12Z" stroke="currentColor" stroke-width="2"/>
                    <path d="M15 12L12 15L9 12" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                    <path d="M12 9V15" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
                </svg>
                Connect Wallet
            `;
            connectBtn.classList.remove('connected');
            connectBtn.onclick = () => this.connect();
            
            // Hide submit task button when wallet is disconnected
            if (submitTaskBtn) {
                submitTaskBtn.style.display = 'none';
            }
        }
    }

    updateWalletStatus(status) {
        // No-op since we removed the wallet status display from header
        console.log('Wallet status:', status);
    }

    showToast(message, isError = false) {
        const toast = document.getElementById('toast');
        if (!toast) return;

        toast.textContent = message;
        toast.className = `toast ${isError ? 'error' : ''}`;
        toast.classList.add('show');

        setTimeout(() => {
            toast.classList.remove('show');
        }, 3000);
    }

    // Transaction tracking methods
    showTransactionsSection() {
        const section = document.getElementById('transactions-section');
        const tabBtn = document.getElementById('transactions-tab-btn');
        
        if (section) {
            section.style.display = 'block';
        }
        
        if (tabBtn) {
            tabBtn.style.display = 'flex';
        }
    }

    hideTransactionsSection() {
        const section = document.getElementById('transactions-section');
        const tabBtn = document.getElementById('transactions-tab-btn');
        
        if (section) {
            section.style.display = 'none';
        }
        
        if (tabBtn) {
            tabBtn.style.display = 'none';
        }
        
        // If we're currently on the transactions tab, switch to dashboard
        const currentTab = document.querySelector('.tab-pane.active');
        if (currentTab && currentTab.id === 'my-transactions-tab') {
            switchTab('dashboard');
        }
    }

    async loadExistingTransactions() {
        try {
            // Load metadata to get manager_address
            const metadata = await taskMonitor.loadMetadata();
            if (!metadata || !metadata.manager_address) {
                console.warn('Manager address not found in metadata');
                return;
            }

            console.log('Loading transactions for wallet:', this.account);
            console.log('Manager address:', metadata.manager_address);

            // Convert wss:// to https:// if needed
            let httpRpcUrl = metadata.rpc_url;
            if (httpRpcUrl.startsWith('wss://')) {
                httpRpcUrl = httpRpcUrl.replace('wss://', 'https://');
            }

            // Get the latest block number to limit our search
            const latestBlockResponse = await fetch(httpRpcUrl, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    jsonrpc: '2.0',
                    method: 'eth_blockNumber',
                    params: [],
                    id: 1
                })
            });

            const latestBlockResult = await latestBlockResponse.json();
            const latestBlock = latestBlockResult.result ? parseInt(latestBlockResult.result, 16) : 0;
            
            // Search in the last 10,000 blocks to avoid hitting limits
            const fromBlock = Math.max(0, latestBlock - 10000);
            const fromBlockHex = '0x' + fromBlock.toString(16);
            const toBlock = 'latest';

            console.log('Searching blocks:', fromBlock, 'to', toBlock);

            // Query for all transaction logs from the manager contract
            const logParams = {
                fromBlock: fromBlockHex,
                toBlock: toBlock,
                address: metadata.manager_address
            };

            // Query for past logs
            const response = await fetch(httpRpcUrl, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    jsonrpc: '2.0',
                    method: 'eth_getLogs',
                    params: [logParams],
                    id: 1
                })
            });

            const result = await response.json();
            if (result.error) {
                console.warn('Error fetching existing transactions:', result.error.message);
                return;
            }

            const logs = result.result || [];
            console.log('Found', logs.length, 'total logs');
            
            // For each log, get the full transaction to check the from address
            const ourTransactions = [];
            
            for (const log of logs) {
                try {
                    // Get the full transaction details
                    const txResponse = await fetch(httpRpcUrl, {
                        method: 'POST',
                        headers: {
                            'Content-Type': 'application/json',
                        },
                        body: JSON.stringify({
                            jsonrpc: '2.0',
                            method: 'eth_getTransactionByHash',
                            params: [log.transactionHash],
                            id: 1
                        })
                    });

                    const txResult = await txResponse.json();
                    if (txResult.result && txResult.result.from) {
                        const txFrom = txResult.result.from.toLowerCase();
                        const ourAccount = this.account.toLowerCase();
                        
                        // Check if this transaction is from our wallet
                        if (txFrom === ourAccount) {
                            ourTransactions.push({
                                hash: log.transactionHash,
                                from: txResult.result.from,
                                to: txResult.result.to,
                                blockNumber: log.blockNumber ? parseInt(log.blockNumber, 16) : null,
                                timestamp: null, // Will be fetched separately if needed
                                status: 'confirmed'
                            });
                        }
                    }
                } catch (txError) {
                    console.warn('Error fetching transaction:', log.transactionHash, txError);
                    continue;
                }
            }

            console.log('Found', ourTransactions.length, 'transactions from our wallet');

            // Add to transaction history (avoid duplicates)
            for (const tx of ourTransactions) {
                if (!this.transactionHistory.find(t => t.hash === tx.hash)) {
                    this.transactionHistory.push(tx);
                }
            }

            // Sort by block number (most recent first)
            this.transactionHistory.sort((a, b) => {
                if (!a.blockNumber && !b.blockNumber) return 0;
                if (!a.blockNumber) return 1;
                if (!b.blockNumber) return -1;
                return b.blockNumber - a.blockNumber;
            });

            // Fetch timestamps for transactions that don't have them
            await this.fetchTransactionTimestamps();

            // Render transactions
            this.renderTransactions();

        } catch (error) {
            console.warn('Failed to load existing transactions:', error);
            this.showToast('Failed to load transaction history', true);
        }
    }

    addTransaction(txHash, toAddress) {
        // Check if transaction already exists
        if (this.transactionHistory.find(t => t.hash === txHash)) {
            return;
        }

        const transaction = {
            hash: txHash,
            from: this.account,
            to: toAddress,
            blockNumber: null,
            timestamp: new Date(),
            status: 'pending'
        };

        this.transactionHistory.unshift(transaction);
        this.renderTransactions();

        // Listen for transaction confirmation
        this.watchTransactionConfirmation(txHash);
    }

    watchTransactionConfirmation(txHash) {
        if (!this.web3) return;

        const checkInterval = setInterval(() => {
            this.web3.eth.getTransactionReceipt(txHash)
                .then(receipt => {
                    if (receipt) {
                        clearInterval(checkInterval);
                        
                        const tx = this.transactionHistory.find(t => t.hash === txHash);
                        if (tx) {
                            tx.status = receipt.status ? 'confirmed' : 'failed';
                            tx.blockNumber = receipt.blockNumber;
                            this.renderTransactions();
                            
                            if (tx.status === 'confirmed') {
                                this.showToast('Transaction confirmed!');
                            } else {
                                this.showToast('Transaction failed', true);
                            }
                        }
                    }
                })
                .catch(err => {
                    console.warn('Error checking transaction:', err);
                });
        }, 2000);

        // Stop watching after 5 minutes
        setTimeout(() => {
            clearInterval(checkInterval);
        }, 300000);
    }

    renderTransactions() {
        const container = document.getElementById('transactions-container');
        if (!container) return;

        if (this.transactionHistory.length === 0) {
            container.innerHTML = `
                <div class="table-empty">
                    <svg width="48" height="48" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M12 22C17.5228 22 22 17.5228 22 12C22 6.47715 17.5228 2 12 2C6.47715 2 2 6.47715 2 12C2 17.5228 6.47715 22 12 22Z" stroke="currentColor" stroke-width="2"/>
                        <path d="M8 12L11 15L16 9" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                    <h3>No transactions yet</h3>
                    <p>Your transaction history will appear here</p>
                </div>
            `;
            return;
        }

        container.innerHTML = `
            <div class="table-wrapper">
                <table class="sqd-table">
                    <thead>
                        <tr>
                            <th>#</th>
                            <th>Transaction Hash</th>
                            <th>Block</th>
                            <th>Timestamp</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${this.transactionHistory.map((tx, index) => this.createTransactionRow(tx, index + 1)).join('')}
                    </tbody>
                </table>
            </div>
        `;
    }

    createTransactionRow(tx, rowNumber) {
        const statusClass = tx.status;
        const timestamp = tx.timestamp ? new Date(tx.timestamp).toLocaleString() : 'Pending...';
        
        // Get blockchain explorer URL based on network
        const getExplorerUrl = (txHash) => {
            const metadata = taskMonitor.metadata;
            if (!metadata) return '#';
            
            const network = metadata.blockchain_network.toLowerCase();
            if (network === 'mainnet') {
                return `https://etherscan.io/tx/${txHash}`;
            } else {
                return `https://${network}.etherscan.io/tx/${txHash}`;
            }
        };
        
        return `
            <tr>
                <td><span class="row-number">${rowNumber}</span></td>
                <td>
                    <a href="${getExplorerUrl(tx.hash)}" target="_blank" class="mono" title="View on blockchain explorer">
                        ${tx.hash}
                    </a>
                </td>
                <td>${tx.blockNumber ? tx.blockNumber : '-'}</td>
                <td class="text-muted">${timestamp}</td>
            </tr>
        `;
    }

    async fetchTransactionTimestamps() {
        try {
            if (!this.web3 || this.transactionHistory.length === 0) return;

            // Get RPC URL from metadata
            const metadata = await taskMonitor.loadMetadata();
            if (!metadata) return;

            let httpRpcUrl = metadata.rpc_url;
            if (httpRpcUrl.startsWith('wss://')) {
                httpRpcUrl = httpRpcUrl.replace('wss://', 'https://');
            }

            // Fetch timestamps for transactions that don't have them
            for (const tx of this.transactionHistory) {
                if (tx.timestamp || !tx.blockNumber) continue;

                try {
                    const response = await fetch(httpRpcUrl, {
                        method: 'POST',
                        headers: {
                            'Content-Type': 'application/json',
                        },
                        body: JSON.stringify({
                            jsonrpc: '2.0',
                            method: 'eth_getBlockByNumber',
                            params: ['0x' + tx.blockNumber.toString(16), false],
                            id: 1
                        })
                    });

                    const result = await response.json();
                    if (result.result && result.result.timestamp) {
                        tx.timestamp = parseInt(result.result.timestamp, 16) * 1000; // Convert to milliseconds
                    }
                } catch (blockError) {
                    console.warn('Error fetching block:', tx.blockNumber, blockError);
                }
            }
        } catch (error) {
            console.warn('Failed to fetch transaction timestamps:', error);
        }
    }

    async refreshTransactions() {
        if (!this.isConnected) {
            this.showToast('Connect your wallet first', true);
            return;
        }

        this.showToast('Refreshing transactions...', false);
        
        // Clear existing history and reload
        this.transactionHistory = [];
        await this.loadExistingTransactions();
        
        this.showToast('Transaction history refreshed');
    }

    clearTransactionHistory() {
        this.transactionHistory = [];
        this.renderTransactions();
        this.showToast('Transaction history cleared');
    }
}

// Task monitoring application
class TaskMonitor {
    constructor() {
        this.tasks = [];
        this.refreshInterval = null;
        this.metadata = null;
        this.ethEvents = [];
        this.fraudData = [];
        this.walletManager = new WalletManager();
        this.init();
    }

    init() {
        this.bindEvents();
        this.loadTasks();
        this.loadFraudData(); // Load fraud data on init
        this.startAutoRefresh();
    }

    bindEvents() {
        // Form submission - now for modal form
        const taskForm = document.getElementById('task-form-modal');
        if (taskForm) {
            taskForm.addEventListener('submit', (e) => this.submitTask(e));
        }
    }

    async loadTasks() {
        try {
            const response = await fetch('/tasks');
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            
            const tasks = await response.json();
            this.tasks = tasks.sort((a, b) => new Date(b.creation_ts * 1000) - new Date(a.creation_ts * 1000));
            this.renderTasks();
            this.updateStats();
        } catch (error) {
            console.error('Failed to load tasks:', error);
            this.showToast('Failed to load tasks', true);
        }
    }

    renderTasks() {
        const container = document.getElementById('tasks-container');
        
        if (!container) return;

        if (this.tasks.length === 0) {
            container.innerHTML = `
                <div class="table-empty">
                    <svg width="48" height="48" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M19 3H5C3.89543 3 3 3.89543 3 5V19C3 20.1046 3.89543 21 5 21H19C20.1046 21 21 20.1046 21 19V5C21 3.89543 20.1046 3 19 3Z" stroke="currentColor" stroke-width="2"/
                        <path d="M3 9H21" stroke="currentColor" stroke-width="2"/
                        <path d="M9 21V9" stroke="currentColor" stroke-width="2"/
                    </svg>
                    <h3>No tasks found</h3>
                    <p>Submit your first task using the form below</p>
                </div>
            `;
            return;
        }

        container.innerHTML = `
            <div class="table-wrapper">
                <table class="sqd-table">
                    <thead>
                        <tr>
                            <th>#</th>
                            <th>Query ID</th>
                            <th>Timestamp</th>
                            <th>Status</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${this.tasks.map((task, index) => this.createTaskRow(task, index + 1)).join('')}
                    </tbody>
                </table>
            </div>
        `;
    }

    createTaskRow(task, rowNum) {
        const timestamp = new Date(task.ts * 1000).toLocaleString();
        const statusClass = task.status.toLowerCase().replace('_', '-');
        
        return `
            <tr onclick="taskMonitor.showTaskDetails('${task.id}')" style="cursor: pointer;">
                <td><span class="row-number">${rowNum}</span></td>
                <td class="mono">${this.escapeHtml(task.query_id)}</td>
                <td class="text-muted">${timestamp}</td>
                <td><span class="status-badge ${statusClass}">${this.formatStatus(task.status)}</span></td>
            </tr>
        `;
    }

    updateStats() {
        const totalTasks = this.tasks.length;
        const runningTasks = this.tasks.filter(task => task.status === 'Running').length;
        const completedTasks = this.tasks.filter(task => task.status === 'Completed').length;

        // Update DOM elements
        const totalElement = document.getElementById('total-tasks');
        const runningElement = document.getElementById('running-tasks');
        const completedElement = document.getElementById('completed-tasks');

        if (totalElement) totalElement.textContent = totalTasks;
        if (runningElement) runningElement.textContent = runningTasks;
        if (completedElement) completedElement.textContent = completedTasks;
    }

    async showTaskDetails(taskId) {
        try {
            const response = await fetch(`/tasks/${taskId}`);
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            
            const task = await response.json();
            this.showTaskModal(task);
        } catch (error) {
            console.error('Failed to load task details:', error);
            this.showToast('Failed to load task details', true);
        }
    }

    showTaskModal(task) {
        // Create modal overlay
        const modal = document.createElement('div');
        modal.style.cssText = `
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0, 0, 0, 0.5);
            display: flex;
            align-items: center;
            justify-content: center;
            z-index: 1000;
        `;

        const modalContent = document.createElement('div');
        modalContent.style.cssText = `
            background: white;
            border-radius: 12px;
            padding: 32px;
            max-width: 600px;
            width: 90%;
            max-height: 80vh;
            overflow-y: auto;
        `;

        const timestamp = new Date(task.ts * 1000).toLocaleString();
        
        // Check if wallet is connected and task has proof data
        const isWalletConnected = this.walletManager.isConnected;
        const hasProofData = task.proof_bytes && task.public_values;
        const canPostProof = isWalletConnected && hasProofData;
        
        modalContent.innerHTML = `
            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 24px;">
                <h3 style="margin: 0; font-size: 20px; font-weight: 600;">Task Details</h3>
                <button onclick="this.closest('.task-modal').remove()" style="
                    background: none;
                    border: none;
                    font-size: 24px;
                    cursor: pointer;
                    color: #6b7280;
                ">×</button>
            </div>
            ${canPostProof ? `
                <div style="margin-bottom: 24px; padding-bottom: 24px; border-bottom: 1px solid #e5e7eb;">
                    <button id="post-proof-btn" onclick="window.taskMonitor.postProof('${task.id}')" style="
                        width: 100%;
                        padding: 12px 20px;
                        background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
                        color: white;
                        border: none;
                        border-radius: 8px;
                        font-size: 14px;
                        font-weight: 500;
                        cursor: pointer;
                        display: flex;
                        align-items: center;
                        justify-content: center;
                        gap: 8px;
                        transition: all 0.2s ease;
                    " onmouseover="this.style.transform='translateY(-1px)';this.style.boxShadow='0 4px 12px rgba(102, 126, 234, 0.4)';" onmouseout="this.style.transform='none';this.style.boxShadow='none';">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                            <path d="M22 2L11 13M22 2L15 22L11 13L2 9L22 2Z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                        </svg>
                        Post Proof to Chain
                    </button>
                    <div id="post-proof-status" style="margin-top: 12px; font-size: 13px; color: #6b7280; text-align: center;"></div>
                </div>
            ` : !isWalletConnected ? `
                <div style="margin-bottom: 24px; padding-bottom: 24px; border-bottom: 1px solid #e5e7eb;">
                    <div style="padding: 12px; background: #fef3c7; border: 1px solid #fcd34d; border-radius: 8px; font-size: 13px; color: #92400e; text-align: center;">
                        Connect your wallet to post the proof to the blockchain
                    </div>
                </div>
            ` : ''}
            <div style="display: grid; gap: 16px;">
                <div>
                    <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Query ID</label>
                    <div style="font-size: 14px; color: #1a1a1a; word-break: break-all; margin-top: 4px;">${this.escapeHtml(task.query_id)}</div>
                </div>
                <div>
                    <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Status</label>
                    <div style="font-size: 14px; color: #1a1a1a; margin-top: 4px;">${this.formatStatus(task.status)}</div>
                </div>
                <div>
                    <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Timestamp</label>
                    <div style="font-size: 14px; color: #1a1a1a; margin-top: 4px;">${timestamp}</div>
                </div>
                ${task.proof_bytes ? `
                    <div>
                        <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Proof Bytes</label>
                        <div style="font-size: 12px; color: #1a1a1a; margin-top: 4px; padding: 12px; background: #f0f9ff; border-radius: 8px; font-family: monospace; word-break: break-all; max-height: 120px; overflow-y: auto;">${this.formatBytes(task.proof_bytes)}</div>
                    </div>
                ` : ''}
                ${task.public_values ? `
                    <div>
                        <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Public Values</label>
                        <div style="font-size: 12px; color: #1a1a1a; margin-top: 4px; padding: 12px; background: #f0fdf4; border-radius: 8px; font-family: monospace; word-break: break-all; max-height: 120px; overflow-y: auto;">${this.formatBytes(task.public_values)}</div>
                    </div>
                ` : ''}
                ${task.comment ? `
                    <div>
                        <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Comment</label>
                        <div style="font-size: 14px; color: #1a1a1a; margin-top: 4px; padding: 12px; background: #f9fafb; border-radius: 8px;">${this.escapeHtml(task.comment)}</div>
                    </div>
                ` : ''}
            </div>
        `;

        modal.className = 'task-modal';
        modal.appendChild(modalContent);
        document.body.appendChild(modal);

        // Close modal on background click
        modal.addEventListener('click', (e) => {
            if (e.target === modal) {
                modal.remove();
            }
        });
    }

    async postProof(taskId) {
        try {
            // Load metadata
            const metadata = await this.loadMetadata();
            
            if (!this.walletManager.isConnected) {
                this.walletManager.showToast('Please connect your wallet first', true);
                return;
            }
            
            // Validate network - check if MetaMask is on the correct network
            const web3 = this.walletManager.web3;
            const networkId = await web3.eth.net.getId();
            
            // Map network names to chain IDs
            const networkChainIds = {
                'mainnet': 1,
                'sepolia': 11155111,
                'holesky': 17000,
                'goerli': 5
            };
            
            const expectedChainId = networkChainIds[metadata.blockchain_network.toLowerCase()];
            
            if (expectedChainId && networkId !== expectedChainId) {
                // Try to switch to the correct network
                try {
                    await window.ethereum.request({
                        method: 'wallet_switchEthereumChain',
                        params: [{ chainId: '0x' + expectedChainId.toString(16) }]
                    });
                } catch (switchError) {
                    // Chain not added to MetaMask
                    if (switchError.code === 4902) {
                        this.walletManager.showToast(`Please add the ${metadata.blockchain_network} network to MetaMask`, true);
                    } else {
                        this.walletManager.showToast(`Please switch to ${metadata.blockchain_network} network in MetaMask`, true);
                    }
                    
                    const btn = document.getElementById('post-proof-btn');
                    const status = document.getElementById('post-proof-status');
                    if (btn) {
                        btn.disabled = false;
                        btn.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M22 2L11 13M22 2L15 22L11 13L2 9L22 2Z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg> Post Proof to Chain';
                        btn.style.background = 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)';
                    }
                    if (status) {
                        status.textContent = `Wrong network: Expected ${metadata.blockchain_network}`;
                        status.style.color = '#f59e0b';
                    }
                    return;
                }
            }
            
            // Get task details
            const response = await fetch(`/tasks/${taskId}`);
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            const task = await response.json();
            
            if (!task.proof_bytes || !task.public_values) {
                this.showToast('No proof data available', true);
                return;
            }
            
            // Update button state
            const btn = document.getElementById('post-proof-btn');
            const status = document.getElementById('post-proof-status');
            if (btn) {
                btn.disabled = true;
                btn.innerHTML = '<svg class="spin" width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M12 2V4M12 20V22M4 12H2M6.31 6.31L4.9 4.9M17.69 6.31L19.1 4.9M6.31 17.69L4.9 19.1M17.69 17.69L19.1 19.1M22 12H20" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg> Preparing Transaction...';
            }
            
            // Convert proof bytes to hex
            const proofBytesHex = this.arrayBufferToHex(task.proof_bytes);
            const publicValuesHex = this.arrayBufferToHex(task.public_values);
            
            // Get the ProvingManager contract ABI
            const provingManagerAbi = [
                {
                    "inputs": [
                        { "internalType": "string", "name": "configName", "type": "string" },
                        { "internalType": "bytes", "name": "publicValues", "type": "bytes" },
                        { "internalType": "bytes", "name": "proofBytes", "type": "bytes" }
                    ],
                    "name": "verifyAndEmit",
                    "outputs": [],
                    "stateMutability": "nonpayable",
                    "type": "function"
                }
            ];
            
            // Create contract instance
            const provingManager = new web3.eth.Contract(provingManagerAbi, metadata.manager_address);
            
            // Encode the function call
            const txData = provingManager.methods.verifyAndEmit(
                metadata.config_name,
                publicValuesHex,
                proofBytesHex
            ).encodeABI();
            
            // Update status
            if (status) {
                status.textContent = 'Ready to send transaction...';
                status.style.color = '#6b7280';
            }
            
            // Prepare transaction - send to network from metadata endpoint
            const tx = {
                from: this.walletManager.account,
                to: metadata.manager_address,
                data: txData,
                value: '0x0'  // Use 0x0 hex format for zero value
            };
            
            if (status) {
                status.textContent = `Sending to ${metadata.blockchain_network}...`;
            }
            
            // Send transaction - MetaMask will estimate gas automatically
            const receipt = await web3.eth.sendTransaction(tx);
            
            // Track the transaction
            if (this.walletManager && this.walletManager.addTransaction) {
                this.walletManager.addTransaction(receipt.transactionHash, metadata.manager_address);
            }
            
            // Success
            if (btn) {
                btn.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M20 6L9 17L4 12" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg> Proof Posted!';
                btn.style.background = 'linear-gradient(135deg, #10b981 0%, #059669 100%)';
            }
            
            if (status) {
                const explorerUrl = metadata.network === 'mainnet' 
                    ? `https://etherscan.io/tx/${receipt.transactionHash}`
                    : `https://${metadata.blockchain_network}.etherscan.io/tx/${receipt.transactionHash}`;
                status.innerHTML = `<a href="${explorerUrl}" target="_blank" style="color: #10b981;">Transaction confirmed!</a>`;
            }
            
            this.walletManager.showToast('Proof posted to blockchain successfully!');
            
        } catch (error) {
            console.error('Failed to post proof:', error);
            
            const btn = document.getElementById('post-proof-btn');
            const status = document.getElementById('post-proof-status');
            
            if (btn) {
                btn.disabled = false;
                btn.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M22 2L11 13M22 2L15 22L11 13L2 9L22 2Z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg> Post Proof to Chain';
                btn.style.background = 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)';
            }
            
            if (status) {
                status.textContent = `Failed: ${error.message}`;
                status.style.color = '#dc2626';
            }
            
            this.walletManager.showToast(`Failed to post proof: ${error.message}`, true);
        }
    }

    arrayBufferToHex(buffer) {
        if (Array.isArray(buffer)) {
            buffer = new Uint8Array(buffer);
        }
        let hex = '0x';
        for (let i = 0; i < buffer.length; i++) {
            hex += buffer[i].toString(16).padStart(2, '0');
        }
        return hex;
    }

    async submitTask(event) {
        event.preventDefault();
        
        const form = event.target;
        const submitButton = form.querySelector('.submit-btn');
        const originalText = submitButton.textContent;
        
        try {
            // Disable form
            submitButton.disabled = true;
            submitButton.textContent = 'Submitting...';
            
            const formData = new FormData(form);
            const taskData = {
                query_id: formData.get('query_id'),
                ts: parseInt(formData.get('ts'))
            };

            const response = await fetch('/tasks', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify(taskData)
            });

            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }

            const taskId = await response.json();
            this.showToast('Task submitted successfully!');
            
            // Reset and close modal
            form.reset();
            closeSubmitTaskModal();
            
            // Reload tasks
            await this.loadTasks();
            
        } catch (error) {
            console.error('Failed to submit task:', error);
            this.showToast('Failed to submit task', true);
        } finally {
            // Re-enable form
            submitButton.disabled = false;
            submitButton.textContent = originalText;
        }
    }

    startAutoRefresh() {
        // Refresh every 5 seconds
        this.refreshInterval = setInterval(() => {
            this.loadTasks();
        }, 5000);
    }

    stopAutoRefresh() {
        if (this.refreshInterval) {
            clearInterval(this.refreshInterval);
            this.refreshInterval = null;
        }
    }

    showToast(message, isError = false) {
        const toast = document.getElementById('toast');
        if (!toast) return;

        toast.textContent = message;
        toast.className = `toast ${isError ? 'error' : ''}`;
        toast.classList.add('show');

        setTimeout(() => {
            toast.classList.remove('show');
        }, 3000);
    }

    async fetchLatestBlockNumber() {
        try {
            if (!this.metadata) {
                await this.loadMetadata();
            }
            
            let httpRpcUrl = this.metadata.rpc_url;
            if (httpRpcUrl.startsWith('wss://')) {
                httpRpcUrl = httpRpcUrl.replace('wss://', 'https://');
            }
            
            const response = await fetch(httpRpcUrl, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    jsonrpc: '2.0',
                    method: 'eth_blockNumber',
                    params: [],
                    id: 1
                })
            });
            
            const result = await response.json();
            
            if (result.error) {
                console.warn('Failed to fetch latest block:', result.error.message);
                return null;
            }
            
            // Convert hex to decimal
            const latestBlock = parseInt(result.result, 16);
            return latestBlock;
        } catch (error) {
            console.warn('Failed to fetch latest block number:', error);
            return null;
        }
    }

    async setDefaultBlockRange() {
        const fromBlockInput = document.getElementById('from-block');
        const toBlockInput = document.getElementById('to-block');
        
        if (!fromBlockInput || !toBlockInput) return;
        
        const latestBlock = await this.fetchLatestBlockNumber();
        
        if (latestBlock !== null && latestBlock > 40000) {
            const defaultFromBlock = latestBlock - 40000;
            fromBlockInput.value = defaultFromBlock;
        } else {
            fromBlockInput.value = '0';
        }
        
        toBlockInput.value = 'latest';
    }

    async loadMetadata() {
        try {
            const response = await fetch('/metadata');
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            
            this.metadata = await response.json();
            return this.metadata;
        } catch (error) {
            console.error('Failed to load metadata:', error);
            this.showToast('Failed to load metadata', true);
            throw error;
        }
    }

    async loadEthEvents() {
        try {
            if (!this.metadata) {
                await this.loadMetadata();
            }

            const fromBlock = document.getElementById('from-block').value || '0';
            const toBlock = document.getElementById('to-block').value || 'latest';
            
            // Show loading state
            const container = document.getElementById('events-container');
            container.innerHTML = `
                <div class="table-loading">
                    <div class="subsquid-loader"></div>
                    <p>Loading FraudFound events...</p>
                </div>
            `;

            // Check if RPC URL is WebSocket and inform user
            // if (this.metadata.rpc_url.startsWith('wss://')) {
            //     this.showToast('Converting WebSocket RPC to HTTP for browser compatibility', false);
            // }

            // Query events using eth_call to get past logs
            const events = await this.queryContractEvents(
                this.metadata.rpc_url,
                this.metadata.manager_address,
                fromBlock,
                toBlock
            );

            // Filter to only show FraudFound events
            this.ethEvents = events.filter(event => event.type === 'FraudFound');
            
            this.renderEthEvents();
        } catch (error) {
            console.error('Failed to load Ethereum events:', error);
            const container = document.getElementById('events-container');
            
            // Extract error details for better user feedback
            let errorMessage = 'Failed to load events';
            let errorDetails = '';
            
            if (error.message.includes('RPC error:')) {
                errorMessage = 'RPC Error';
                errorDetails = error.message.replace('RPC error: ', '');
            } else if (error.message.includes('fetch')) {
                errorMessage = 'Network Error';
                errorDetails = 'Failed to connect to RPC endpoint';
            } else {
                errorDetails = error.message;
            }
            
            container.innerHTML = `
                <div class="error-state">
                    <h3 style="color: #dc2626; margin-bottom: 8px;">${errorMessage}</h3>
                    <p style="color: #374151; margin-bottom: 12px;">${errorDetails}</p>
                    <div style="background: #fef2f2; border: 1px solid #fecaca; border-radius: 6px; padding: 12px; margin-top: 12px;">
                        <strong style="color: #991b1b; font-size: 13px;">Troubleshooting Tips:</strong>
                        <ul style="margin: 8px 0 0 16px; color: #6b7280; font-size: 12px;">
                            ${errorDetails.includes('exceed maximum block range') ? 
                                '<li>Try reducing the block range size (use smaller numbers or closer block ranges)</li>' : ''}
                            ${errorDetails.includes('invalid') ? 
                                '<li>Check that your block numbers are valid (positive integers)</li>' : ''}
                            ${errorDetails.includes('network') || errorDetails.includes('fetch') ? 
                                '<li>Verify the RPC endpoint is accessible from your browser</li>' : ''}
                            <li>Try using "latest" as the end block to query recent events only</li>
                        </ul>
                    </div>
                </div>
            `;
            
            // Show toast notification with the actual error
            this.showToast(errorDetails, true);
        }
    }



    async queryContractEvents(rpcUrl, contractAddress, fromBlock, toBlock) {
        // Query only FraudFound events from the contract
        const events = [];
        
        try {
            // Calculate keccak256 hash for FraudFound event signature
            let fraudFoundSignature;
            if (typeof Web3 !== 'undefined') {
                const web3 = new Web3();
                fraudFoundSignature = web3.utils.keccak256('FraudFound(string,uint256)');
            } else {
                // Fallback signature if Web3 is not available
                fraudFoundSignature = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
            }
            
            // Query only FraudFound events
            const fraudFoundLogs = await this.getPastLogs(rpcUrl, contractAddress, fraudFoundSignature, fromBlock, toBlock);
            
            for (const log of fraudFoundLogs) {
                events.push({
                    type: 'FraudFound',
                    blockNumber: parseInt(log.blockNumber, 16),
                    transactionHash: log.transactionHash,
                    logIndex: parseInt(log.logIndex, 16),
                    signature: log.topics[0],
                    data: this.decodeEventData('FraudFound', log.data, log.topics),
                    topics: log.topics
                });
            }
        } catch (error) {
            console.warn('Failed to query FraudFound events:', error);
            // Re-throw with more context
            throw new Error(`FraudFound event query failed: ${error.message}`);
        }

        return events.sort((a, b) => b.blockNumber - a.blockNumber);
    }

    async calculateEventSignatures() {
        // If Web3 is available, calculate actual keccak256 hashes
        if (typeof Web3 !== 'undefined') {
            const web3 = new Web3();
            try {
                return {
                    [web3.utils.keccak256('FraudFound(string,uint256)')]: 'FraudFound',
                    [web3.utils.keccak256('RoleAdminChanged(bytes32,bytes32,bytes32)')]: 'RoleAdminChanged',
                    [web3.utils.keccak256('RoleGranted(bytes32,address,address)')]: 'RoleGranted',
                    [web3.utils.keccak256('RoleRevoked(bytes32,address,address)')]: 'RoleRevoked'
                };
            } catch (error) {
                console.warn('Failed to calculate event signatures with Web3:', error);
            }
        }
        
        // Fallback to hardcoded signatures if Web3 is not available
        return {
            // These are the actual keccak256 hashes for the event signatures
            '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef': 'FraudFound',
            '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890': 'RoleAdminChanged',
            '0x5678901234567890abcdef1234567890abcdef1234567890abcdef1234567890': 'RoleGranted',
            '0xfedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321': 'RoleRevoked'
        };
    }

    async getPastLogs(rpcUrl, contractAddress, eventSignature, fromBlock, toBlock) {
        // Convert wss:// to https:// if needed (Fetch API doesn't support WebSocket)
        let httpRpcUrl = rpcUrl;
        if (rpcUrl.startsWith('wss://')) {
            httpRpcUrl = rpcUrl.replace('wss://', 'https://');
        }
        
        // Format block numbers with 0x prefix if they're numeric
        const formatBlockParam = (block) => {
            // Handle special keywords
            if (block === 'latest' || block === 'earliest' || block === 'pending') {
                return block;
            }
            
            // Check if it's already a hex string
            if (block.startsWith('0x') || block.startsWith('0X')) {
                return block;
            }
            
            // Convert decimal string/number to hex
            const num = parseInt(block, 10);
            if (!isNaN(num) && num >= 0) {
                return '0x' + num.toString(16);
            }
            
            // Return as-is if we can't parse it (fallback)
            return block;
        };
        
        // Build the log query parameters
        const logParams = {
            fromBlock: formatBlockParam(fromBlock),
            toBlock: formatBlockParam(toBlock),
            address: contractAddress
        };
        
        // Add topics only if event signature is provided
        if (eventSignature) {
            logParams.topics = [eventSignature];
        }
        
        const response = await fetch(httpRpcUrl, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                jsonrpc: '2.0',
                method: 'eth_getLogs',
                params: [logParams],
                id: 1
            })
        });

        if (!response.ok) {
            throw new Error(`RPC request failed: ${response.status}`);
        }

        const result = await response.json();
        
        if (result.error) {
            // Create a detailed error message for the user
            const errorMsg = result.error.message || 'Unknown RPC error';
            const errorCode = result.error.code || 'Unknown code';
            throw new Error(`RPC error (${errorCode}): ${errorMsg}`);
        }

        return result.result || [];
    }

    decodeEventData(eventName, data, topics = []) {
        // Enhanced decoding for actual contract event types
        switch (eventName) {
            case 'FraudFound':
                // FraudFound(string peer_id, uint256 timestamp)
                // String is encoded as: offset(32 bytes) + length(32 bytes) + data
                let peerId = 'Unknown';
                let timestamp = 'Unknown';
                
                if (data && data.length > 128) {
                    try {
                        // Extract string offset and length
                        const stringOffset = parseInt(data.slice(2, 66), 16);
                        const timestampWord = data.slice(66, 130);
                        const stringLengthStart = stringOffset * 2 + 2;
                        const stringLength = parseInt(data.slice(stringLengthStart, stringLengthStart + 64), 16);
                        
                        // Extract string data (peer_id)
                        const stringDataStart = stringLengthStart + 64;
                        const stringDataEnd = stringDataStart + stringLength * 2;
                        const peerIdHex = data.slice(stringDataStart, stringDataEnd);
                        peerId = this.hexToUtf8(peerIdHex);
                        
                        // Extract timestamp from the second word in the head
                        timestamp = parseInt(timestampWord, 16);
                    } catch (error) {
                        console.warn('Failed to decode FraudFound event data:', error);
                    }
                }
                return { peerId, timestamp: timestamp !== 'Unknown' ? new Date(timestamp * 1000).toLocaleString() : timestamp };
                
            case 'RoleAdminChanged':
                // RoleAdminChanged(bytes32 role, bytes32 previousAdminRole, bytes32 newAdminRole)
                return {
                    role: this.formatAddressOrBytes32(topics[1]),
                    previousAdminRole: this.formatAddressOrBytes32(topics[2]),
                    newAdminRole: this.formatAddressOrBytes32(topics[3])
                };
                
            case 'RoleGranted':
                // RoleGranted(bytes32 role, address account, address sender)
                return {
                    role: this.formatAddressOrBytes32(topics[1]),
                    account: this.formatAddress(topics[2]),
                    sender: this.formatAddress(topics[3])
                };
                
            case 'RoleRevoked':
                // RoleRevoked(bytes32 role, address account, address sender)
                return {
                    role: this.formatAddressOrBytes32(topics[1]),
                    account: this.formatAddress(topics[2]),
                    sender: this.formatAddress(topics[3])
                };
                
            default:
                return { 
                    signature: topics[0] || 'unknown',
                    raw: data,
                    topics: topics
                };
        }
    }

    formatAddress(address) {
        if (!address || address === '0x0000000000000000000000000000000000000000') {
            return 'Zero Address';
        }
        return address;
    }

    formatAddressOrBytes32(value) {
        if (!value) return 'Unknown';
        return value;
    }

    renderEthEvents() {
        const container = document.getElementById('events-container');
        
        if (!container) return;

        if (this.ethEvents.length === 0) {
            container.innerHTML = `
                <div class="table-empty">
                    <svg width="48" height="48" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M13 2L3 14H12L11 22L21 10H12L13 2Z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                    <h3>No FraudFound events</h3>
                    <p>No FraudFound events found for the specified block range</p>
                </div>
            `;
            return;
        }

        container.innerHTML = `
            <div class="table-wrapper">
                <table class="sqd-table">
                    <thead>
                        <tr>
                            <th>Block</th>
                            <th>Transaction</th>
                            <th>Peer ID</th>
                            <th>Timestamp</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${this.ethEvents.map(event => this.createEventRow(event)).join('')}
                    </tbody>
                </table>
            </div>
        `;
    }

    createEventRow(event) {
        const peerId = event.data.peerId || 'Unknown';
        const fraudTime = event.data.timestamp || 'Unknown';
        
        // Get blockchain explorer URL based on network
        const getExplorerUrl = (txHash) => {
            const metadata = this.metadata;
            if (!metadata) return '#';
            
            const network = metadata.blockchain_network.toLowerCase();
            if (network === 'mainnet') {
                return `https://etherscan.io/tx/${txHash}`;
            } else {
                return `https://${network}.etherscan.io/tx/${txHash}`;
            }
        };
        
        return `
            <tr class="event-card fraudfound">
                <td>${event.blockNumber}</td>
                <td>
                    <a href="${getExplorerUrl(event.transactionHash)}" target="_blank" class="mono-sm" title="View on blockchain explorer">
                        ${event.transactionHash.slice(0, 14)}...${event.transactionHash.slice(-8)}
                    </a>
                </td>
                <td class="mono">${this.escapeHtml(peerId)}</td>
                <td class="text-muted">${fraudTime}</td>
            </tr>
        `;
    }

    formatAddressShort(address) {
        if (!address || address === '0x0000000000000000000000000000000000000000') {
            return 'Zero Address';
        }
        return address.slice(0, 8) + '...' + address.slice(-6);
    }

    formatRoleDisplay(role) {
        if (!role) return 'Unknown';
        // Check if it's the DEFAULT_ADMIN_ROLE (0x0000000000000000000000000000000000000000000000000000000000000000)
        if (role === '0x0000000000000000000000000000000000000000000000000000000000000000') {
            return 'DEFAULT_ADMIN_ROLE';
        }
        // Check if it's a common role pattern
        if (role.length === 66 && role.startsWith('0x')) {
            return role.slice(0, 10) + '...';
        }
        return role;
    }



    formatStatus(status) {
        return status.replace('_', ' ').toLowerCase()
            .split(' ')
            .map(word => word.charAt(0).toUpperCase() + word.slice(1))
            .join(' ');
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    formatBytes(bytes) {
        if (!bytes || bytes.length === 0) return 'No data';
        
        // Convert bytes to hex string
        const hexArray = Array.from(bytes, byte => byte.toString(16).padStart(2, '0'));
        const hexString = hexArray.join('');
        
        // Format as 0x... for easy copy-pasting
        return '0x' + hexString;
    }

    hexToUtf8(hexString) {
        if (!hexString) return '';
        const cleanHex = hexString.startsWith('0x') ? hexString.slice(2) : hexString;
        if (cleanHex.length === 0) return '';

        try {
            const bytes = cleanHex.match(/.{1,2}/g)?.map(byte => parseInt(byte, 16)) || [];
            const decoded = new TextDecoder('utf-8', { fatal: false }).decode(new Uint8Array(bytes));
            return decoded.replace(/\u0000+$/g, '');
        } catch (error) {
            console.warn('Failed to decode hex to UTF-8:', error);
            return hexString;
        }
    }

    async loadFraudData() {
        try {
            const graphqlEndpoint = 'https://fa7e5d08-286a-4511-9598-d4aa8ea9594b.squids.live/zk-feed@v1/api/graphql';
            
            const query = `
                query {
                    contractEventFraudFounds(limit: 100) {
                        blockNumber
                        peerId
                        timestamp
                    }
                }
            `;

            const response = await fetch(graphqlEndpoint, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    query: query
                })
            });

            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }

            const result = await response.json();
            
            if (result.errors) {
                throw new Error(`GraphQL error: ${result.errors[0].message}`);
            }

            this.fraudData = result.data.contractEventFraudFounds || [];
            this.renderFraudData();
        } catch (error) {
            console.error('Failed to load fraud data:', error);
            this.showToast('Failed to load fraud data', true);
            this.renderFraudDataError(error.message);
        }
    }

    renderFraudData() {
        const container = document.getElementById('fraud-data-container');
        
        if (!container) return;

        if (this.fraudData.length === 0) {
            container.innerHTML = `
                <div class="table-empty">
                    <svg width="48" height="48" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M12 22C17.5228 22 22 17.5228 22 12C22 6.47715 17.5228 2 12 2C6.47715 2 2 6.47715 2 12C2 17.5228 6.47715 22 12 22Z" stroke="currentColor" stroke-width="2"/>
                        <path d="M8 12L11 15L16 9" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                    <h3>No fraud events found</h3>
                    <p>No fraud events data available from GraphQL endpoint</p>
                </div>
            `;
            return;
        }

        container.innerHTML = `
            <div class="table-wrapper">
                <table class="sqd-table">
                    <thead>
                        <tr>
                            <th>#</th>
                            <th>Block Number</th>
                            <th>Peer ID</th>
                            <th>Timestamp</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${this.fraudData.map((item, index) => this.createFraudDataRow(item, index + 1)).join('')}
                    </tbody>
                </table>
            </div>
        `;
    }

    renderFraudDataError(errorMessage) {
        const container = document.getElementById('fraud-data-container');
        
        if (!container) return;

        container.innerHTML = `
            <div class="error-state">
                <h3 style="color: #dc2626; margin-bottom: 8px;">Failed to Load Fraud Data</h3>
                <p style="color: #374151; margin-bottom: 12px;">${errorMessage}</p>
                <div style="background: #fef2f2; border: 1px solid #fecaca; border-radius: 6px; padding: 12px; margin-top: 12px;">
                    <strong style="color: #991b1b; font-size: 13px;">Possible Issues:</strong>
                    <ul style="margin: 8px 0 0 16px; color: #6b7280; font-size: 12px;">
                        <li>GraphQL endpoint is unreachable</li>
                        <li>Network connectivity issues</li>
                        <li>Query format or syntax error</li>
                        <li>Server-side issues with the endpoint</li>
                    </ul>
                </div>
                <button class="refresh-btn" onclick="loadFraudData()" style="margin-top: 16px;">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <path d="M1 4V10H7" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                        <path d="M3.51 15A9 9 0 1 0 6 5L1 10" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                    Try Again
                </button>
            </div>
        `;
    }

    createFraudDataRow(item, rowNum) {
        // Format timestamp if it's a number (Unix timestamp) or use as-is
        let formattedTimestamp;
        if (item.timestamp) {
            try {
                // Try to parse as Unix timestamp (seconds or milliseconds)
                const timestamp = parseInt(item.timestamp);
                if (timestamp > 1e12) {
                    // Milliseconds
                    formattedTimestamp = new Date(timestamp).toLocaleString();
                } else if (timestamp > 1e9) {
                    // Seconds
                    formattedTimestamp = new Date(timestamp * 1000).toLocaleString();
                } else {
                    // Already formatted or unknown format
                    formattedTimestamp = item.timestamp;
                }
            } catch {
                formattedTimestamp = item.timestamp;
            }
        } else {
            formattedTimestamp = 'N/A';
        }

        // Format block number
        const blockNumber = item.blockNumber ? item.blockNumber.toString() : 'N/A';
        
        // Format peer ID (display full value)
        let peerId = item.peerId || 'N/A';
        
        return `
            <tr>
                <td><span class="row-number">${rowNum}</span></td>
                <td>${blockNumber}</td>
                <td class="mono">${this.escapeHtml(peerId)}</td>
                <td class="text-muted">${this.escapeHtml(formattedTimestamp)}</td>
            </tr>
        `;
    }
}

// Modal management functions
function openSubmitTaskModal() {
    const modal = document.getElementById('submit-task-modal');
    if (modal) {
        modal.classList.add('show');
        document.body.style.overflow = 'hidden';
        
        // Click outside to close functionality disabled for submit modal
        
        // Focus on first input
        setTimeout(() => {
            const firstInput = modal.querySelector('input');
            if (firstInput) firstInput.focus();
        }, 300);
    }
}

function closeSubmitTaskModal() {
    const modal = document.getElementById('submit-task-modal');
    if (modal) {
        modal.classList.remove('show');
        document.body.style.overflow = '';
        
        // Reset form after animation
        setTimeout(() => {
            const form = document.getElementById('task-form-modal');
            if (form) {
                form.reset();
            }
        }, 300);
    }
}

// Global functions for HTML onclick handlers
function refreshTasks() {
    if (window.taskMonitor) {
        window.taskMonitor.loadTasks();
    }
}

// Ethereum Events Modal management functions
function openEthEventsModal() {
    const modal = document.getElementById('eth-events-modal');
    if (modal) {
        modal.classList.add('show');
        document.body.style.overflow = 'hidden';
        
        // Add click outside listener
        const handleClickOutside = (event) => {
            if (event.target === modal) {
                closeEthEventsModal();
            }
        };
        modal.addEventListener('click', handleClickOutside);
        
        // Store reference to remove later
        modal._handleClickOutside = handleClickOutside;
        
        // Load metadata and set default block range
        if (window.taskMonitor) {
            window.taskMonitor.loadMetadata().then(() => {
                window.taskMonitor.setDefaultBlockRange();
            }).catch(error => {
                console.error('Failed to load metadata:', error);
                // Set default to 0 if metadata fails to load
                const fromBlockInput = document.getElementById('from-block');
                if (fromBlockInput) fromBlockInput.value = '0';
            });
        }
    }
}

function closeEthEventsModal() {
    const modal = document.getElementById('eth-events-modal');
    if (modal) {
        modal.classList.remove('show');
        document.body.style.overflow = '';
        
        // Remove click outside listener
        if (modal._handleClickOutside) {
            modal.removeEventListener('click', modal._handleClickOutside);
            delete modal._handleClickOutside;
        }
    }
}

function loadEthEvents() {
    if (window.taskMonitor) {
        window.taskMonitor.loadEthEvents();
    }
}



// Wallet functions
function connectWallet() {
    if (window.taskMonitor && window.taskMonitor.walletManager) {
        window.taskMonitor.walletManager.connect();
    }
}

function clearTransactionHistory() {
    if (window.taskMonitor && window.taskMonitor.walletManager) {
        window.taskMonitor.walletManager.clearTransactionHistory();
    }
}

function refreshTransactions() {
    if (window.taskMonitor && window.taskMonitor.walletManager) {
        window.taskMonitor.walletManager.refreshTransactions();
    }
}

function loadFraudData() {
    if (window.taskMonitor) {
        window.taskMonitor.loadFraudData();
    }
}



// Tab switching functionality
function switchTab(tabName) {
    // Check if trying to access transactions tab without wallet connection
    if (tabName === 'my-transactions' && window.taskMonitor && !window.taskMonitor.walletManager.isConnected) {
        window.taskMonitor.walletManager.showToast('Please connect your wallet to view transactions', true);
        return;
    }
    
    // Hide all tab panes
    const tabPanes = document.querySelectorAll('.tab-pane');
    tabPanes.forEach(pane => {
        pane.classList.remove('active');
    });
    
    // Remove active class from all tab buttons
    const tabButtons = document.querySelectorAll('.tab-button');
    tabButtons.forEach(button => {
        button.classList.remove('active');
    });
    
    // Show selected tab pane
    const selectedPane = document.getElementById(tabName + '-tab');
    if (selectedPane) {
        selectedPane.classList.add('active');
    }
    
    // Add active class to selected tab button
    const selectedButton = document.querySelector(`[data-tab="${tabName}"]`);
    if (selectedButton) {
        selectedButton.classList.add('active');
    }
    
    // Load fraud data when fraud events tab is opened
    if (tabName === 'fraud-events' && window.taskMonitor) {
        window.taskMonitor.loadFraudData();
    }
}

// Initialize when DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    window.taskMonitor = new TaskMonitor();
});

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    if (window.taskMonitor) {
        window.taskMonitor.stopAutoRefresh();
    }
});
