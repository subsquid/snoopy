// Task monitoring application
class TaskMonitor {
    constructor() {
        this.tasks = [];
        this.refreshInterval = null;
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