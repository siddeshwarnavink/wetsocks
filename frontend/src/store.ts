import { Message, StoredMessage } from "./types";

const IDB_KEY = "MessageDatabase";
const MESSAGES_KEY = "messages";
const MAX_MESSAGES = 100;
const NULL_GROUP_ID = "__NULL_GROUP__";

class MessageStore {
    private db: IDBDatabase | null = null;

    async init(): Promise<void> {
        return new Promise((resolve, reject) => {
            const request = indexedDB.open(IDB_KEY, 1);

            request.onerror = () => reject(request.error);
            request.onsuccess = () => {
                this.db = request.result;
                resolve();
            };

            request.onupgradeneeded = (event) => {
                const db = (event.target as IDBOpenDBRequest).result;

                if (!db.objectStoreNames.contains(MESSAGES_KEY)) {
                    const objectStore = db.createObjectStore(MESSAGES_KEY, {
                        keyPath: 'id',
                        autoIncrement: true
                    });

                    objectStore.createIndex('groupId', 'groupId', { unique: false });
                    objectStore.createIndex('timestamp', 'timestamp', { unique: false });
                    objectStore.createIndex('is_unread', 'is_unread', { unique: false });
                }
            };
        });
    }

    private ensureDb(): IDBDatabase {
        if (!this.db) {
            throw new Error('Database not initialized. Call init() first.');
        }
        return this.db;
    }

    private normalizeGroupId(groupId: string | null): string {
        return groupId === null ? NULL_GROUP_ID : groupId;
    }

    private denormalizeMessage(msg: any): StoredMessage {
        return {
            ...msg,
            groupId: msg.groupId === NULL_GROUP_ID ? null : msg.groupId
        };
    }

    async appendMessage(message: Message, is_unread: boolean = false): Promise<number> {
        const db = this.ensureDb();
        return new Promise(async (resolve, reject) => {
            try {
                const existingMessages = await this.getMessagesByGroupId(message.groupId);

                const transaction = db.transaction([MESSAGES_KEY], 'readwrite');
                const store = transaction.objectStore(MESSAGES_KEY);

                if (existingMessages.length >= MAX_MESSAGES) {
                    const oldestMessage = existingMessages[0];
                    if (oldestMessage.id !== undefined) {
                        store.delete(oldestMessage.id);
                    }
                }

                const storedMessage: StoredMessage = {
                    sender: message.sender,
                    payload: message.payload,
                    groupId: this.normalizeGroupId(message.groupId),
                    timestamp: Date.now(),
                    is_unread
                };

                const request = store.add(storedMessage);

                request.onsuccess = () => resolve(request.result as number);
                request.onerror = () => reject(request.error);
            } catch (error) {
                reject(error);
            }
        });
    }

    async getMessagesByGroupId(groupId: string | null): Promise<StoredMessage[]> {
        const db = this.ensureDb();

        return new Promise((resolve, reject) => {
            const transaction = db.transaction([MESSAGES_KEY], 'readonly');
            const store = transaction.objectStore(MESSAGES_KEY);
            const index = store.index('groupId');

            const normalizedGroupId = this.normalizeGroupId(groupId);
            const request = index.getAll(normalizedGroupId);

            request.onsuccess = () => {
                const messages = request.result.map(msg => this.denormalizeMessage(msg));
                messages.sort((a, b) => a.timestamp - b.timestamp);
                resolve(messages);
            };
            request.onerror = () => reject(request.error);
        });
    }

    async getAllMessages(): Promise<StoredMessage[]> {
        const db = this.ensureDb();

        return new Promise((resolve, reject) => {
            const transaction = db.transaction([MESSAGES_KEY], 'readonly');
            const store = transaction.objectStore(MESSAGES_KEY);
            const request = store.getAll();

            request.onsuccess = () => {
                const messages = request.result.map(msg => this.denormalizeMessage(msg));
                messages.sort((a, b) => a.timestamp - b.timestamp);
                resolve(messages);
            };
            request.onerror = () => reject(request.error);
        });
    }

    async clearGroup(groupId: string | null): Promise<void> {
        const db = this.ensureDb();

        return new Promise((resolve, reject) => {
            const transaction = db.transaction([MESSAGES_KEY], 'readwrite');
            const store = transaction.objectStore(MESSAGES_KEY);
            const index = store.index('groupId');

            const normalizedGroupId = this.normalizeGroupId(groupId);
            const request = index.openCursor(normalizedGroupId);

            request.onsuccess = (event) => {
                const cursor = (event.target as IDBRequest).result;
                if (cursor) {
                    cursor.delete();
                    cursor.continue();
                } else {
                    resolve();
                }
            };
            request.onerror = () => reject(request.error);
        });
    }

    async clearAllMessages(): Promise<void> {
        const db = this.ensureDb();

        return new Promise((resolve, reject) => {
            const transaction = db.transaction([MESSAGES_KEY], 'readwrite');
            const store = transaction.objectStore(MESSAGES_KEY);
            const request = store.clear();

            request.onsuccess = () => resolve();
            request.onerror = () => reject(request.error);
        });
    }

    async markMessagesAsRead(groupId: string | null): Promise<void> {
        const db = this.ensureDb();

        return new Promise((resolve, reject) => {
            const transaction = db.transaction([MESSAGES_KEY], 'readwrite');
            const store = transaction.objectStore(MESSAGES_KEY);
            const index = store.index('groupId');

            const normalizedGroupId = this.normalizeGroupId(groupId);
            const request = index.openCursor(normalizedGroupId);

            request.onsuccess = (event) => {
                const cursor = (event.target as IDBRequest).result;
                if (cursor) {
                    const message = cursor.value;
                    if (message.is_unread) {
                        message.is_unread = false;
                        cursor.update(message);
                    }
                    cursor.continue();
                } else {
                    resolve();
                }
            };
            request.onerror = () => reject(request.error);
        });
    }

    async hasUnreadMessages(groupId: string | null): Promise<boolean> {
        const db = this.ensureDb();

        return new Promise((resolve, reject) => {
            const transaction = db.transaction([MESSAGES_KEY], 'readonly');
            const store = transaction.objectStore(MESSAGES_KEY);
            const index = store.index('groupId');

            const normalizedGroupId = this.normalizeGroupId(groupId);
            const request = index.openCursor(normalizedGroupId);

            request.onsuccess = (event) => {
                const cursor = (event.target as IDBRequest).result;
                if (cursor) {
                    const message = cursor.value;
                    if (message.is_unread) {
                        resolve(true);
                        return;
                    }
                    cursor.continue();
                } else {
                    resolve(false);
                }
            };
            request.onerror = () => reject(request.error);
        });
    }
}

export const messageStore = new MessageStore();
