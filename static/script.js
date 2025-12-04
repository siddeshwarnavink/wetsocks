const messages = document.getElementById("messages");
const input = document.getElementById("message_input");
const btn = document.getElementById("message_btn");

const socket = new WebSocket("ws://localhost:3333/ws");

socket.onopen = function(event) {
  console.log("onopen", event);
  btn.addEventListener("click", () => {
    socket.send(input.value);
  });
};

socket.onmessage = function(event) {
  console.log("onmessage", event);
  messages.innerHTML += `<p>${event.data}</p>`;
};

socket.onclose = function(event) {
  console.log("onclose", event);
};

