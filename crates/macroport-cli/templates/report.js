// SPDX-License-Identifier: Apache-2.0
// Filters the files by what is typed in the box, and opens or closes them all.
// It reads text and sets properties. It builds no markup and makes no request.
(function () {
  "use strict";
  var toolbar = document.getElementById("toolbar");
  var filter = document.getElementById("filter");
  var shown = document.getElementById("shown");
  var expand = document.getElementById("expand");
  var collapse = document.getElementById("collapse");
  var files = document.querySelectorAll("details.file");
  if (!toolbar || !filter || !shown || !expand || !collapse) {
    return;
  }

  // What the closed line of each file shows: its path and the value of each
  // chip, without the word that says what the chip is about. The first summary
  // of a file is its own: the modules and procedures inside have theirs.
  var rows = [];
  for (var i = 0; i < files.length; i++) {
    var parts = files[i].querySelector("summary").querySelectorAll(".path, .value");
    var words = [];
    for (var j = 0; j < parts.length; j++) {
      words.push(parts[j].textContent);
    }
    rows.push(words.join(" ").toLowerCase());
  }

  function update() {
    var needle = filter.value.trim().toLowerCase();
    var count = 0;
    for (var i = 0; i < files.length; i++) {
      var match = needle === "" || rows[i].indexOf(needle) !== -1;
      files[i].hidden = !match;
      if (match) {
        count++;
      }
    }
    shown.textContent = count + " / " + files.length;
  }

  function setOpen(open) {
    for (var i = 0; i < files.length; i++) {
      if (!files[i].hidden) {
        files[i].open = open;
        var inner = files[i].querySelectorAll("details");
        for (var j = 0; j < inner.length; j++) {
          inner[j].open = open;
        }
      }
    }
  }

  filter.addEventListener("input", update);
  expand.addEventListener("click", function () { setOpen(true); });
  collapse.addEventListener("click", function () { setOpen(false); });
  toolbar.hidden = false;
  update();
})();
