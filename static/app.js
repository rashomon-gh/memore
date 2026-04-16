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

        // Cytoscape instance
        cy: null,

        // Chart instances
        charts: {},

        // Chat
        chatMessages: [],
        chatInput: '',
        chatLoading: false,
        currentChatId: null,
        chatHistory: [],

        async init() {
            await this.loadStats();
            await this.loadGraphData();
            await this.loadChatHistory();

            this.$watch('activeTab', (value) => {
                if (value === 'analytics') {
                    this.$nextTick(() => this.loadAnalytics());
                }
                if (value === 'chat') {
                    this.loadChatHistory();
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

        async sendChat() {
            const message = this.chatInput.trim();
            if (!message || this.chatLoading) return;

            this.chatMessages.push({ role: 'user', text: message, memories: [], opinions: [] });
            this.chatInput = '';
            this.chatLoading = true;

            try {
                const response = await fetch(`${API_BASE}/chat`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ message, chat_id: this.currentChatId })
                });

                if (!response.ok) throw new Error('Chat request failed');

                const data = await response.json();
                this.currentChatId = data.chat_id;

                this.chatMessages.push({
                    role: 'assistant',
                    text: data.response,
                    memories: data.new_memories || [],
                    opinions: data.opinions || []
                });

                await Promise.all([this.loadStats(), this.loadChatHistory()]);
            } catch (error) {
                console.error('Chat error:', error);
                this.chatMessages.push({
                    role: 'assistant',
                    text: 'Sorry, something went wrong. Please try again.',
                    memories: [],
                    opinions: []
                });
            } finally {
                this.chatLoading = false;
                this.$nextTick(() => {
                    const el = document.getElementById('chatMessages');
                    if (el) el.scrollTop = el.scrollHeight;
                });
            }
        },

        async loadChatHistory() {
            try {
                const response = await fetch(`${API_BASE}/chats`);
                if (!response.ok) return;
                this.chatHistory = await response.json();
            } catch (error) {
                console.error('Failed to load chat history:', error);
            }
        },

        async loadChat(chatId) {
            try {
                const response = await fetch(`${API_BASE}/chats/${chatId}`);
                if (!response.ok) return;
                const data = await response.json();

                this.currentChatId = data.id;
                this.chatMessages = data.messages.map(m => ({
                    role: m.role,
                    text: m.content,
                    memories: [],
                    opinions: []
                }));

                if (data.memories && data.memories.length > 0) {
                    const lastAssistantIdx = this.chatMessages.map((m, i) => m.role === 'assistant' ? i : -1).filter(i => i >= 0).pop();
                    if (lastAssistantIdx !== undefined) {
                        this.chatMessages[lastAssistantIdx].memories = data.memories.filter(m => m.network !== 'opinion');
                        this.chatMessages[lastAssistantIdx].opinions = data.memories.filter(m => m.network === 'opinion');
                    }
                }

                this.$nextTick(() => {
                    const el = document.getElementById('chatMessages');
                    if (el) el.scrollTop = el.scrollHeight;
                });
            } catch (error) {
                console.error('Failed to load chat:', error);
            }
        },

        newChat() {
            this.currentChatId = null;
            this.chatMessages = [];
        },

        async deleteChat(chatId) {
            try {
                const response = await fetch(`${API_BASE}/chats/${chatId}`, { method: 'DELETE' });
                if (!response.ok) return;

                if (this.currentChatId === chatId) {
                    this.newChat();
                }
                await Promise.all([this.loadStats(), this.loadChatHistory()]);
            } catch (error) {
                console.error('Failed to delete chat:', error);
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
        }
    };
}

// Initialize the app when Alpine.js is ready
document.addEventListener('alpine:init', () => {
    Alpine.data('hindsightApp', hindsightApp);
});
