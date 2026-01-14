import { User } from "./types";
import * as utils from "./utils";
import * as ws from "./wasm/crypto_wasm.js";
import { messageStore } from "./store";

const PROFILE_KEY = "profile";

const welcome_dialog = document.getElementById("welcome_dialog") as HTMLDialogElement | null;
const messages = document.getElementById("messages");
const user_list = document.getElementById("user-list");
const group_chat_item = document.querySelector('.chat-item[data-chat-id="group"]') as HTMLElement | null;
const message_form = document.getElementById("message_form") as HTMLFormElement | null;

// Global state
let socket: WebSocket | null = null;
let profile: User | null = null;
let groupId: string | null = null;
const users: { [id: string]: User } = {};

async function update_users_list() {
    if (!user_list) return;
    user_list.innerHTML = "";

    for (const id of Object.keys(users)) {
        const user = users[id];
        const color = utils.name_color(user.name);
        const isActive = groupId === user.public_key ? 'active' : '';
        const hasUnread = await messageStore.hasUnreadMessages(user.public_key);
        const unreadIndicator = hasUnread ? '<span class="unread-indicator"></span>' : '';

        user_list.innerHTML += `
        <div class="chat-item ${isActive}" data-chat-id="${user.public_key}">
            <div class="avatar online" style="background: ${color};">
              ${user.name.charAt(0).toUpperCase()}
            </div>
            <div class="chat-info">
                <div class="chat-name">${user.name}</div>
                <div class="chat-status">Online</div>
            </div>
            ${unreadIndicator}
        </div>
        `;
    }

    const chatItems = user_list.querySelectorAll('.chat-item');
    chatItems.forEach(item => {
        item.addEventListener('click', () => {
            const chatId = item.getAttribute('data-chat-id');
            select_chat(chatId);
        });
    });

    const hasGroupUnread = await messageStore.hasUnreadMessages(null);
    if (group_chat_item) {
        const existingIndicator = group_chat_item.querySelector('.unread-indicator');
        if (hasGroupUnread && !existingIndicator) {
            group_chat_item.innerHTML += '<span class="unread-indicator"></span>';
        } else if (!hasGroupUnread && existingIndicator) {
            existingIndicator.remove();
        }
    }
}

function select_chat(chatId: string | null) {
    groupId = chatId;

    document.querySelectorAll('.chat-item').forEach(item => {
        item.classList.remove('active');
    });

    if (chatId === null) {
        group_chat_item?.classList.add('active');
    } else {
        const selectedItem = document.querySelector(`.chat-item[data-chat-id="${chatId}"]`);
        selectedItem?.classList.add('active');
    }

    load_stored_messages();
    messageStore.markMessagesAsRead(chatId).then(() => {
        update_users_list();
    });
}

function append_user_message(name: string, text: string): void {
    const color = utils.name_color(name);
    if (messages) {
        messages.innerHTML += `
        <div class="user-message">
            <b style="color:${color}">${name}: </b>
            ${text}
        </div>
        `;
    }
}

function append_server_message(text: string): void {
    if (messages) {
        messages.innerHTML += `
            <div class="server-message">${text}</div>
        `;
    }
}

async function load_stored_messages() {
    if (messages) messages.innerHTML = "";

    const msgs = await messageStore.getMessagesByGroupId(groupId);
    console.log("Stored messages", {
        groupId,
        msgs
    });

    msgs.forEach(({ sender, payload }) => {
        append_user_message(sender, payload);
    });
}

async function on_message(event: MessageEvent) {
    if (profile == null) return;

    const msg = JSON.parse(event.data);
    console.log(msg);

    switch (msg.kind) {
        case "new_user":
            users[msg.user.public_key] = msg.user;
            append_server_message(`${msg.user.name} joined the chat.`);
            update_users_list();
            break;
        case "relay_message":
            const user = users[msg.sender];
            const text = ws.decrypt_message(msg.payload, profile.private_key);

            let gid = msg.group_id;
            if (gid && gid == profile.public_key) gid = user.public_key;
            else gid = null;

            await messageStore.appendMessage({
                sender: user.name,
                payload: text,
                groupId: gid
            }, groupId !== gid);

            if (groupId === gid) append_user_message(user.name, text);
            else update_users_list();
            break;
        case "user_left":
            delete users[msg.user_id];
            // const name = users[msg.user_id].name;
            // append_server_message(`${name} left the chat.`);
            update_users_list();
            break;
    }
}

function ws_setup() {
    socket = new WebSocket("/ws");

    socket.onopen = () => {
        if (socket == null) return;
        if (profile == null) return;
        if (message_form == null) return;

        socket.send(JSON.stringify({
            kind: "first",
            public_key: profile.public_key,
            name: profile.name
        }));

        message_form.addEventListener("submit", (event: SubmitEvent) => {
            event.preventDefault();

            if (event.target == null) return;
            if (profile == null) return;

            const text = message_form.getElementsByTagName("input")[0].value;
            append_user_message(profile.name, text);
            messageStore.appendMessage({
                sender: profile.name,
                payload: text,
                groupId
            });

            Object.keys(users).forEach(user_public_key => {
                const user = users[user_public_key];
                const payload = ws.encrypt_message(text, user.public_key);
                if (socket) {
                    socket.send(JSON.stringify({
                        kind: "send_message",
                        recipient: user.public_key,
                        payload,
                        group_id: groupId
                    }));
                }
            });

            (event.target as HTMLFormElement).reset();
        });
    };

    socket.onmessage = on_message;

    groupId = null;
    load_stored_messages();

    // Setup group chat click handler
    if (group_chat_item) {
        group_chat_item.addEventListener('click', () => {
            select_chat(null);
        });
    }
}

function on_welcome(event: SubmitEvent) {
    if (event.target == null) return;
    if (profile == null) return;
    const welcome_form = event.target as HTMLFormElement;

    profile.name = welcome_form.getElementsByTagName("input")[0].value;
    localStorage.setItem(PROFILE_KEY, JSON.stringify(profile));

    ws_setup();

    append_server_message(`${profile.name} joined the chat.`);
}

messageStore.init().then(() => {
    const profile_json = localStorage.getItem(PROFILE_KEY);
    if (profile_json) {
        profile = JSON.parse(profile_json);
        ws_setup();
    } else if (welcome_dialog) {
        const { public_key, private_key } = JSON.parse(ws.generate_keypair()) as User;
        profile = {
            id: public_key,
            name: "John Doe",
            public_key,
            private_key
        }
        const form = welcome_dialog.getElementsByTagName("form")[0];
        form.addEventListener("submit", on_welcome);
        welcome_dialog.showModal();
    }
});
