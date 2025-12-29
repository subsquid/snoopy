// Task monitoring application
class TaskMonitor {
    constructor() {
        this.tasks = [];
        this.refreshInterval = null;
        this.metadata = null;
        this.ethEvents = [];
        this.init();
    }

    init() {
        this.bindEvents();
        this.loadTasks();
        this.startAutoRefresh();
    }

    bindEvents() {
        // Form submission - now for modal form
        const taskForm = document.getElementById('task-form-modal');
        if (taskForm) {
            taskForm.addEventListener('submit', (e) => this.submitTask(e));
        }

        // Set default timestamp in modal
        const timestampInput = document.getElementById('modal-timestamp');
        if (timestampInput) {
            timestampInput.value = Math.floor(Date.now() / 1000);
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
                <div class="empty-state">
                    <h3>No tasks found</h3>
                    <p>Submit your first task using the form below</p>
                </div>
            `;
            return;
        }

        container.innerHTML = this.tasks.map(task => this.createTaskCard(task)).join('');
    }

    createTaskCard(task) {
        const timestamp = new Date(task.ts * 1000).toLocaleString();
        const statusClass = task.status.toLowerCase().replace('_', '-');
        const commentClass = task.status.toLowerCase();
        
        return `
            <div class="task-card" onclick="taskMonitor.showTaskDetails('${task.id}')">
                <div class="task-details">
                    <div class="task-detail">
                        <div class="task-detail-label">Query ID</div>
                        <div class="task-detail-value">${this.escapeHtml(task.query_id)}</div>
                    </div>
                    <div class="task-detail">
                        <div class="task-detail-label">Timestamp</div>
                        <div class="task-detail-value">${timestamp}</div>
                    </div>
                    <div class="task-detail">
                        <div class="task-detail-label">Status</div>
                        <div class="task-detail-value">${this.formatStatus(task.status)}</div>
                    </div>
                </div>
                ${task.comment ? `
                    <div class="task-comment ${commentClass}">
                        ${this.escapeHtml(task.comment)}
                    </div>
                ` : ''}
            </div>
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
            <div style="display: grid; gap: 16px;">
                <div>
                    <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Query ID</label>
                    <div style="font-size: 14px; color: #1a1a1a; word-break: break-all; margin-top: 4px;">${this.escapeHtml(task.query_id)}</div>
                </div>
                <div>
                    <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Task ID</label>
                    <div style="font-size: 14px; color: #1a1a1a; margin-top: 4px;">${task.id}</div>
                </div>
                <div>
                    <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Status</label>
                    <div style="font-size: 14px; color: #1a1a1a; margin-top: 4px;">${this.formatStatus(task.status)}</div>
                </div>
                <div>
                    <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Timestamp</label>
                    <div style="font-size: 14px; color: #1a1a1a; margin-top: 4px;">${timestamp}</div>
                </div>
                ${task.comment ? `
                    <div>
                        <label style="font-size: 12px; font-weight: 500; color: #6b7280; text-transform: uppercase; letter-spacing: 0.05em;">Comment</label>
                        <div style="font-size: 14px; color: #1a1a1a; margin-top: 4px; padding: 12px; background: #f9fafb; border-radius: 8px;">${this.escapeHtml(task.comment)}</div>
                    </div>
                ` : ''}
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
            document.getElementById('modal-timestamp').value = Math.floor(Date.now() / 1000);
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
                <div class="loading-state">
                    <div class="subsquid-loader"></div>
                    <p>Loading Ethereum events...</p>
                </div>
            `;

            // Check if RPC URL is WebSocket and inform user
            if (this.metadata.rpc_url.startsWith('wss://')) {
                this.showToast('Converting WebSocket RPC to HTTP for browser compatibility', false);
            }

            // Query events using eth_call to get past logs
            const events = await this.queryContractEvents(
                this.metadata.rpc_url,
                this.metadata.manager_address,
                fromBlock,
                toBlock
            );

            this.ethEvents = events;
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
        // Query all events from the contract without filtering by specific event signatures
        // This is a more reliable approach that doesn't depend on exact keccak256 hashes
        const events = [];
        
        try {
            const allLogs = await this.getPastLogs(rpcUrl, contractAddress, null, fromBlock, toBlock);
            
            for (const log of allLogs) {
                // Determine event type based on the first topic (event signature)
                const eventSignature = log.topics[0];
                let eventType = 'Unknown';
                
                // Map common event signatures to event types
                // Note: These are example mappings - in production you'd want exact keccak256 hashes
                const eventSignatureMap = {
                    // RoleAdminChanged, RoleGranted, RoleRevoked would have their actual keccak256 hashes here
                    '0x0000000000000000000000000000000000000000000000000000000000000000': 'RoleAdminChanged'
                };
                
                eventType = eventSignatureMap[eventSignature] || 'ContractEvent';
                
                events.push({
                    type: eventType,
                    blockNumber: parseInt(log.blockNumber, 16),
                    transactionHash: log.transactionHash,
                    logIndex: parseInt(log.logIndex, 16),
                    signature: eventSignature,
                    data: this.decodeEventData(eventType, log.data, log.topics),
                    topics: log.topics
                });
            }
        } catch (error) {
            console.warn('Failed to query contract events:', error);
            // Re-throw with more context
            throw new Error(`Event query failed: ${error.message}`);
        }

        return events.sort((a, b) => b.blockNumber - a.blockNumber);
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
        // Basic decoding for common event types
        switch (eventName) {
            case 'FraudFound':
                return { peerId: '0x' + data.slice(2, 66), timestamp: parseInt(data.slice(66), 16) };
            case 'RoleAdminChanged':
            case 'RoleGranted':
            case 'RoleRevoked':
                return {
                    role: topics[1] || '0x' + data.slice(2, 66),
                    account: topics[2] || '0x' + data.slice(66, 106),
                    sender: topics[3] || '0x' + data.slice(106, 146)
                };
            default:
                return { 
                    signature: topics[0] || 'unknown',
                    raw: data,
                    topics: topics
                };
        }
    }

    renderEthEvents() {
        const container = document.getElementById('events-container');
        
        if (!container) return;

        if (this.ethEvents.length === 0) {
            container.innerHTML = `
                <div class="empty-state">
                    <h3>No events found</h3>
                    <p>No events found for the specified block range</p>
                </div>
            `;
            return;
        }

        container.innerHTML = this.ethEvents.map(event => this.createEventCard(event)).join('');
    }

    createEventCard(event) {
        const timestamp = new Date(event.blockNumber * 1000).toLocaleString();
        const eventTypeClass = event.type.toLowerCase();
        
        let eventDataHtml = '';
        
        switch (event.type) {
            case 'FraudFound':
                eventDataHtml = `
                    <div class="event-data">
                        <div class="event-detail">
                            <span class="event-detail-label">Peer ID:</span>
                            <span class="event-detail-value">${event.data.peerId}</span>
                        </div>
                        <div class="event-detail">
                            <span class="event-detail-label">Timestamp:</span>
                            <span class="event-detail-value">${new Date(event.data.timestamp * 1000).toLocaleString()}</span>
                        </div>
                    </div>
                `;
                break;
            case 'RoleAdminChanged':
            case 'RoleGranted':
            case 'RoleRevoked':
                eventDataHtml = `
                    <div class="event-data">
                        <div class="event-detail">
                            <span class="event-detail-label">Role:</span>
                            <span class="event-detail-value">${event.data.role || 'N/A'}</span>
                        </div>
                        <div class="event-detail">
                            <span class="event-detail-label">Account:</span>
                            <span class="event-detail-value">${event.data.account || 'N/A'}</span>
                        </div>
                        <div class="event-detail">
                            <span class="event-detail-label">Sender:</span>
                            <span class="event-detail-value">${event.data.sender || 'N/A'}</span>
                        </div>
                    </div>
                `;
                break;
            default:
                eventDataHtml = `
                    <div class="event-data">
                        <div class="event-detail">
                            <span class="event-detail-label">Signature:</span>
                            <span class="event-detail-value">${(event.data.signature || 'unknown').slice(0, 10)}...</span>
                        </div>
                        <div class="event-detail">
                            <span class="event-detail-label">Data:</span>
                            <span class="event-detail-value">${event.data.raw ? event.data.raw.slice(0, 20) + '...' : 'No data'}</span>
                        </div>
                    </div>
                `;
        }
        
        return `
            <div class="event-card ${eventTypeClass}">
                <div class="event-header">
                    <div class="event-type">${event.type}</div>
                    <div class="event-block">Block #${event.blockNumber}</div>
                </div>
                ${eventDataHtml}
                <div class="event-footer">
                    <div class="event-tx">TX: ${event.transactionHash.slice(0, 10)}...</div>
                    <div class="event-time">${timestamp}</div>
                </div>
            </div>
        `;
    }

    filterEthEvents(eventType) {
        if (eventType === 'all') {
            this.renderEthEvents();
            return;
        }

        const filteredEvents = this.ethEvents.filter(event => event.type === eventType);
        const originalEvents = this.ethEvents;
        this.ethEvents = filteredEvents;
        this.renderEthEvents();
        this.ethEvents = originalEvents;
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
}

// Modal management functions
function openSubmitTaskModal() {
    const modal = document.getElementById('submit-task-modal');
    if (modal) {
        // Set default timestamp
        const timestampInput = document.getElementById('modal-timestamp');
        if (timestampInput && !timestampInput.value) {
            timestampInput.value = Math.floor(Date.now() / 1000);
        }
        
        modal.classList.add('show');
        document.body.style.overflow = 'hidden';
        
        // Add click outside listener
        const handleClickOutside = (event) => {
            if (event.target === modal) {
                closeSubmitTaskModal();
            }
        };
        modal.addEventListener('click', handleClickOutside);
        
        // Store reference to remove later
        modal._handleClickOutside = handleClickOutside;
        
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
        
        // Remove click outside listener
        if (modal._handleClickOutside) {
            modal.removeEventListener('click', modal._handleClickOutside);
            delete modal._handleClickOutside;
        }
        
        // Reset form after animation
        setTimeout(() => {
            const form = document.getElementById('task-form-modal');
            if (form) {
                form.reset();
                const timestampInput = document.getElementById('modal-timestamp');
                if (timestampInput) {
                    timestampInput.value = Math.floor(Date.now() / 1000);
                }
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
        
        // Load metadata if not already loaded
        if (window.taskMonitor && !window.taskMonitor.metadata) {
            window.taskMonitor.loadMetadata().catch(error => {
                console.error('Failed to load metadata:', error);
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

function filterEvents() {
    const eventFilter = document.getElementById('event-filter');
    if (eventFilter && window.taskMonitor) {
        window.taskMonitor.filterEthEvents(eventFilter.value);
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