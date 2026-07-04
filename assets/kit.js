(function () {
  var root = document.documentElement;
  var storageKey = "glance-theme";
  var choices = Array.prototype.slice.call(document.querySelectorAll("[data-theme-choice]"));

  function apply(choice) {
    if (choice === "light" || choice === "dark") {
      root.setAttribute("data-theme", choice);
      try { localStorage.setItem(storageKey, choice); } catch (_) {}
    } else {
      root.removeAttribute("data-theme");
      try { localStorage.removeItem(storageKey); } catch (_) {}
      choice = "system";
    }
    choices.forEach(function (button) {
      var active = button.getAttribute("data-theme-choice") === choice;
      button.setAttribute("aria-pressed", active ? "true" : "false");
    });
  }

  try {
    var saved = localStorage.getItem(storageKey);
    if (saved === "light" || saved === "dark") {
      root.setAttribute("data-theme", saved);
    }
  } catch (_) {}

  choices.forEach(function (button) {
    button.addEventListener("click", function () {
      apply(button.getAttribute("data-theme-choice"));
    });
  });
  apply(root.getAttribute("data-theme") || "system");

  document.addEventListener("click", function (event) {
    var cite = event.target.closest && event.target.closest(".glance-cite");
    document.querySelectorAll(".glance-cite.is-open").forEach(function (node) {
      if (node !== cite) node.classList.remove("is-open");
    });
    if (!cite) return;
    cite.classList.toggle("is-open");
  });
})();
