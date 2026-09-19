const button = document.getElementById("load-commits");
const list = document.getElementById("commit-list");

if (!button || !list) {
  throw new Error("Missing #load-commits or #commit-list element");
}

function formatDate(iso) {
  const date = new Date(iso);
  if (isNaN(date.getTime())) {
    return iso;
  }
  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

function render(commits) {
  list.textContent = "";
  for (const commit of commits) {
    const li = document.createElement("li");

    const hash = document.createElement("code");
    hash.className = "commit-hash";
    hash.textContent = commit.sha.slice(0, 7);

    const date = document.createElement("span");
    date.className = "commit-date";
    date.textContent = formatDate(commit.commit.author.date);

    const message = document.createElement("span");
    message.className = "commit-message";
    message.textContent = commit.commit.message.split("\n")[0];

    li.append(hash, " ", date, " ", message);
    list.appendChild(li);
  }
}

function fail(message) {
  list.textContent = "";
  const li = document.createElement("li");
  li.className = "commit-error";
  li.textContent = message;
  list.appendChild(li);
}

button.addEventListener("click", async () => {
  try {
    const res = await fetch(
      "https://api.github.com/repos/francoisromain/bundla/commits?per_page=10",
    );
    if (!res.ok) {
      throw new Error(`HTTP ${res.status}`);
    }
    render(await res.json());
  } catch (error) {
    console.error("Failed to load commits:", error);
    fail("Could not load commits. Is the server reachable and CORS allowed?");
  }
});