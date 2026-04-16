// Hindsight Memory Bank Dashboard - Main Application

const API_BASE = '/api';

function hindsightApp() {
    return {
        activeTab: 'graph',
        stats: {
            total_memories: 0,
            total_edges: 0,
            memories_by_network: { world: 0, experience: 0, opinion: 0, observation: 0 },
            edges_by_type: { temporal: 0, semantic: 0, entity: 0, causal: 0 },
            top_entities: [],
            recent_memories: 0,
            average_confidence: null
        },

        // Graph filters
        graphFilters: {
            network: ''
        },

        // Search functionality
        searchQuery: '',
        searchFilters: {
            network: ''
        },
        searchResults: [],
        searchPerformed: false,

        // Memory inspection
        selectedMemory: null,
        relatedMemories: [],

        // Chat functionality
        chatMessages: [],
        chatInput: '',
        chatStatus: {
            typing: false,
            uploading: false,
            uploadMessage: '',
            uploadProgress: 0
        },
        chatFile: {
            selected: false,
            name: '',
            size: 0,
            valid: false,
            file: null
        },

        // Cytoscape instance
        cy: null,

        // Chart instances
        charts: {},

        async init() {
            await this.loadStats();
            await this.loadGraphData();

            // Initialize chat with welcome message
            this.addChatMessage('assistant', 'Hello! I\'m Hindsight, your AI memory assistant. You can:\n\n• Chat with me to build memories from our conversations\n• Upload PDF files to extract knowledge\n• Share Markdown documents for memory building\n\nTry uploading a document or just start talking!');

            // Load analytics when tab is clicked
            this.$watch('activeTab', (value) => {
                if (value === 'analytics') {
                    this.$nextTick(() => this.loadAnalytics());
                }
            });
        },

        async loadStats() {
            try {
                const response = await fetch(`${API_BASE}/stats`);
                this.stats = await response.json();
            } catch (error) {
                console.error('Failed to load stats:', error);
            }
        },

        async loadGraphData() {
            try {
                const params = new URLSearchParams();
                if (this.graphFilters.network) {
                    params.append('network', this.graphFilters.network);
                }

                const response = await fetch(`${API_BASE}/graph?${params}`);
                const data = await response.json();

                this.initCytoscape(data);
            } catch (error) {
                console.error('Failed to load graph data:', error);
            }
        },

        initCytoscape(data) {
            if (this.cy) {
                this.cy.destroy();
            }

            this.cy = cytoscape({
                container: document.getElementById('cy'),
                elements: [
                    ...data.nodes.map(node => ({
                        data: {
                            id: node.data.id,
                            label: node.data.label,
                            network: node.data.network,
                            entities: node.data.entities,
                            confidence: node.data.confidence
                        }
                    })),
                    ...data.edges.map(edge => ({
                        data: {
                            id: edge.data.id,
                            source: edge.data.source,
                            target: edge.data.target,
                            type: edge.data.type,
                            weight: edge.data.weight
                        }
                    }))
                ],
                style: [
                    {
                        selector: 'node',
                        style: {
                            'label': 'data(label)',
                            'width': '30px',
                            'height': '30px',
                            'font-size': '8px',
                            'text-valign': 'center',
                            'text-halign': 'center',
                            'background-color': (ele) => this.getNetworkColor(ele.data('network')),
                            'border-width': 2,
                            'border-color': '#fff'
                        }
                    },
                    {
                        selector: 'node:selected',
                        style: {
                            'border-width': 4,
                            'border-color': '#3b82f6'
                        }
                    },
                    {
                        selector: 'edge',
                        style: {
                            'width': (ele) => Math.max(1, ele.data('weight') * 3),
                            'line-color': (ele) => this.getEdgeColor(ele.data('type')),
                            'curve-style': 'bezier',
                            'opacity': 0.6
                        }
                    }
                ],
                layout: {
                    name: 'cose',
                    animate: false,
                    nodeRepulsion: 8000,
                    idealEdgeLength: 50,
                    nodeDimensionsIncludeLabels: true
                }
            });

            // Add click handler for nodes
            this.cy.on('tap', 'node', (evt) => {
                const node = evt.target;
                const memoryId = node.id();
                this.inspectMemory(memoryId);
            });
        },

        getNetworkColor(network) {
            const colors = {
                world: '#3b82f6',
                experience: '#22c55e',
                opinion: '#f97316',
                observation: '#a855f7'
            };
            return colors[network] || '#6b7280';
        },

        getEdgeColor(type) {
            const colors = {
                temporal: '#ef4444',
                semantic: '#3b82f6',
                entity: '#22c55e',
                causal: '#f59e0b'
            };
            return colors[type] || '#9ca3af';
        },

        async searchMemories() {
            try {
                const params = new URLSearchParams();
                params.append('search', this.searchQuery);
                if (this.searchFilters.network) {
                    params.append('network', this.searchFilters.network);
                }

                const response = await fetch(`${API_BASE}/memories?${params}`);
                const data = await response.json();

                this.searchResults = data.memories;
                this.searchPerformed = true;
            } catch (error) {
                console.error('Failed to search memories:', error);
                this.searchResults = [];
                this.searchPerformed = true;
            }
        },

        async inspectMemory(memoryId) {
            try {
                const response = await fetch(`${API_BASE}/memories/${memoryId}`);
                const data = await response.json();

                this.selectedMemory = data.memory;
                this.relatedMemories = data.neighbors;
                this.activeTab = 'inspector';
            } catch (error) {
                console.error('Failed to load memory details:', error);
            }
        },

        loadAnalytics() {
            this.loadNetworkChart();
            this.loadEdgeChart();
            this.loadEntityChart();
        },

        loadNetworkChart() {
            const ctx = document.getElementById('networkChart');
            if (!ctx) return;

            if (this.charts.network) {
                this.charts.network.destroy();
            }

            this.charts.network = new Chart(ctx, {
                type: 'pie',
                data: {
                    labels: ['World', 'Experience', 'Opinion', 'Observation'],
                    datasets: [{
                        data: [
                            this.stats.memories_by_network.world,
                            this.stats.memories_by_network.experience,
                            this.stats.memories_by_network.opinion,
                            this.stats.memories_by_network.observation
                        ],
                        backgroundColor: ['#3b82f6', '#22c55e', '#f97316', '#a855f7']
                    }]
                },
                options: {
                    responsive: true,
                    plugins: {
                        legend: {
                            position: 'bottom'
                        }
                    }
                }
            });
        },

        loadEdgeChart() {
            const ctx = document.getElementById('edgeChart');
            if (!ctx) return;

            if (this.charts.edge) {
                this.charts.edge.destroy();
            }

            this.charts.edge = new Chart(ctx, {
                type: 'doughnut',
                data: {
                    labels: ['Temporal', 'Semantic', 'Entity', 'Causal'],
                    datasets: [{
                        data: [
                            this.stats.edges_by_type.temporal,
                            this.stats.edges_by_type.semantic,
                            this.stats.edges_by_type.entity,
                            this.stats.edges_by_type.causal
                        ],
                        backgroundColor: ['#ef4444', '#3b82f6', '#22c55e', '#f59e0b']
                    }]
                },
                options: {
                    responsive: true,
                    plugins: {
                        legend: {
                            position: 'bottom'
                        }
                    }
                }
            });
        },

        loadEntityChart() {
            const ctx = document.getElementById('entityChart');
            if (!ctx) return;

            if (this.charts.entity) {
                this.charts.entity.destroy();
            }

            const topEntities = this.stats.top_entities.slice(0, 10);

            this.charts.entity = new Chart(ctx, {
                type: 'bar',
                data: {
                    labels: topEntities.map(e => e.entity),
                    datasets: [{
                        label: 'References',
                        data: topEntities.map(e => e.count),
                        backgroundColor: '#3b82f6'
                    }]
                },
                options: {
                    responsive: true,
                    indexAxis: 'y',
                    plugins: {
                        legend: {
                            display: false
                        }
                    },
                    scales: {
                        x: {
                            beginAtZero: true
                        }
                    }
                }
            });
        },

        formatDate(dateString) {
            const date = new Date(dateString);
            const now = new Date();
            const diffMs = now - date;
            const diffMins = Math.floor(diffMs / 60000);
            const diffHours = Math.floor(diffMs / 3600000);
            const diffDays = Math.floor(diffMs / 86400000);

            if (diffMins < 1) return 'Just now';
            if (diffMins < 60) return `${diffMins}m ago`;
            if (diffHours < 24) return `${diffHours}h ago`;
            if (diffDays < 7) return `${diffDays}d ago`;

            return date.toLocaleDateString();
        },

        // Chat functionality
        async sendMessage() {
            // Handle file upload if file is selected
            if (this.chatFile.selected && this.chatFile.valid) {
                await this.uploadChatFile();
                return;
            }

            // Handle text message
            const message = this.chatInput.trim();
            if (!message) return;

            // Add user message to chat
            this.addChatMessage('user', message);
            this.chatInput = '';

            // Show typing indicator
            this.chatStatus.typing = true;

            try {
                // For now, just simulate a response since we don't have a chat API yet
                setTimeout(() => {
                    this.chatStatus.typing = false;
                    this.addChatMessage('assistant', 'I received your message! The chat functionality is currently being integrated with the backend. You can upload PDF and Markdown files using the attachment button, and I\'ll extract memories from them.');
                }, 1000);

            } catch (error) {
                this.chatStatus.typing = false;
                this.addChatMessage('assistant', 'Sorry, I encountered an error processing your message. Please try again.');
                console.error('Chat error:', error);
            }
        },

        addChatMessage(sender, content, memoriesCreated = null) {
            const message = {
                id: Date.now() + Math.random(),
                sender: sender,
                content: content,
                timestamp: new Date().toISOString(),
                memories_created: memoriesCreated
            };

            this.chatMessages.push(message);

            // Scroll to bottom of chat
            this.$nextTick(() => {
                const chatContainer = document.getElementById('chatMessages');
                if (chatContainer) {
                    chatContainer.scrollTop = chatContainer.scrollHeight;
                }
            });
        },

        handleChatFileSelect(event) {
            const file = event.target.files[0];
            if (!file) return;

            // Validate file type (only PDF and Markdown)
            const validExtensions = ['.pdf', '.md', '.markdown'];
            const fileExtension = '.' + file.name.split('.').pop().toLowerCase();
            const isValid = validExtensions.includes(fileExtension);

            // Validate file size (50MB limit)
            const maxSize = 50 * 1024 * 1024; // 50MB
            const isValidSize = file.size <= maxSize;

            this.chatFile = {
                selected: true,
                name: file.name,
                size: file.size,
                valid: isValid && isValidSize,
                file: file
            };

            // Auto-send if file is valid
            if (this.chatFile.valid) {
                this.uploadChatFile();
            }
        },

        async uploadChatFile() {
            if (!this.chatFile.selected || !this.chatFile.valid || !this.chatFile.file) return;

            const file = this.chatFile.file;

            // Add file upload message to chat
            this.addChatMessage('user', `📎 Uploading file: ${file.name}`);

            // Reset chat file state
            this.chatFile = {
                selected: false,
                name: '',
                size: 0,
                valid: false,
                file: null
            };

            // Show upload progress
            this.chatStatus.uploading = true;
            this.chatStatus.uploadMessage = `Processing ${file.name}...`;
            this.chatStatus.uploadProgress = 0;

            try {
                // Simulate progress (since we can't track real upload progress easily)
                const progressInterval = setInterval(() => {
                    if (this.chatStatus.uploadProgress < 90) {
                        this.chatStatus.uploadProgress += 10;
                    }
                }, 500);

                // Create FormData
                const formData = new FormData();
                formData.append('file', file);

                // Upload file
                const response = await fetch(`${API_BASE}/files/upload`, {
                    method: 'POST',
                    body: formData
                });

                clearInterval(progressInterval);

                if (!response.ok) {
                    throw new Error('File upload failed');
                }

                const result = await response.json();

                // Update progress to complete
                this.chatStatus.uploadProgress = 100;
                this.chatStatus.uploadMessage = 'Complete!';

                // Add success message to chat
                this.addChatMessage('assistant',
                    `✅ Successfully processed ${result.filename}!\n\nCreated ${result.memories_created} memories from the document.\nProcessing time: ${result.processing_time_ms}ms`,
                    result.memories_created);

                // Refresh stats
                await this.loadStats();

                // Hide upload progress after a delay
                setTimeout(() => {
                    this.chatStatus.uploading = false;
                    this.chatStatus.uploadProgress = 0;
                    this.chatStatus.uploadMessage = '';
                }, 3000);

            } catch (error) {
                this.chatStatus.uploading = false;
                this.chatStatus.uploadProgress = 0;
                this.chatStatus.uploadMessage = '';

                // Add error message to chat
                this.addChatMessage('assistant',
                    `❌ Failed to process ${file.name}. Please make sure it's a valid PDF or Markdown file and try again.`);
                console.error('File upload error:', error);
            }
        },

        clearChatFile() {
            this.chatFile = {
                selected: false,
                name: '',
                size: 0,
                valid: false,
                file: null
            };
        },

        formatTime(timestamp) {
            const date = new Date(timestamp);
            return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        }
    };
}

// Initialize the app when Alpine.js is ready
document.addEventListener('alpine:init', () => {
    Alpine.data('hindsightApp', hindsightApp);
});
