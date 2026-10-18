# Consigne Claude Code : fabriquer les classeurs de test dans la VM Windows (Parallels)

À coller dans Claude Code lancé **sur le Mac**, à la racine du dépôt `macro-port`.

Prérequis côté humain, à faire avant :

- la VM Windows 11 est démarrée et ta session Windows est ouverte ;
- Excel pour Windows est installé dans la VM, activé et ouvert au moins une fois ;
- Parallels Desktop est en édition Pro ou Business, la seule qui fournit la commande `prlctl` ;
- les Parallels Tools sont installés dans la VM ;
- le partage du dossier personnel du Mac vers Windows est activé (Configuration de la VM > Options > Partage).

---

Tu vas fabriquer les classeurs de test binaires de Macro-Port dans ma VM Windows 11 Parallels, en la pilotant depuis le Mac avec `prlctl`. Lis d'abord `docs/CORPUS-PLAN.md` (points 1 à 5), `tools/fixtures/Build-Fixtures.ps1`, `tools/fixtures/make_malformed.py` et `corpus/fixtures/README.md`.

## Règles impératives

- N'exécute jamais une macro des classeurs. Le script désactive l'exécution des macros (AutomationSecurity) : ne change pas ce comportement.
- Ne modifie ni les échantillons texte de `corpus/families/`, ni `corpus/manifest.json`, ni la vérité terrain.
- Ne désactive jamais l'antivirus de la VM. Une seule exclusion est autorisée, limitée au dossier `C:\macro-port\corpus\fixtures`, et tu la retires à la fin.
- Si une étape demande un mot de passe, une connexion à un compte Microsoft, une activation de licence ou une validation dans une fenêtre, arrête-toi et demande-moi.
- Pour les commandes PowerShell non triviales, écris un fichier `.ps1` temporaire dans un dossier partagé et exécute-le. N'envoie pas de longues commandes en une ligne, à cause des problèmes de guillemets.
- Tiens un journal de chaque commande lancée et de son résultat, pour me le restituer à la fin.

## 1. Vérifications préalables (arrête-toi et explique-moi le problème si l'une échoue)

1. `prlctl --version` répond. Sinon, Parallels n'est probablement pas en édition Pro ou Business.
2. `prlctl list -a` : identifie la VM Windows 11 et vérifie qu'elle est démarrée (`running`). S'il y a plusieurs VM Windows, demande-moi laquelle utiliser.
3. `prlctl exec "<VM>" --current-user cmd /c "whoami && ver"` répond avec mon utilisateur Windows. Cela confirme que les Parallels Tools sont présents et que ma session est ouverte.
4. `prlctl exec "<VM>" cmd /c whoami` (sans `--current-user`) : note sous quel compte la commande tourne et si elle a les droits administrateur. Tu en auras besoin à l'étape 3.
5. Depuis la VM, le dépôt du Mac est accessible par le partage Parallels (en général `\\Mac\Home\<chemin du dépôt sous mon dossier personnel>`). Vérifie-le avec `Test-Path`.
6. Excel est utilisable par automatisation : avec `--current-user`, un petit script PowerShell crée `Excel.Application`, ajoute un classeur, l'enregistre en `.xlsx` dans `C:\macro-port-check\test.xlsx`, ferme Excel et vérifie que le fichier existe. Un échec à l'enregistrement signale souvent une licence non activée : arrête-toi.
7. Windows PowerShell 5.1 est présent (`powershell.exe`, `$PSVersionTable.PSVersion.Major` égal à 5).

## 2. Copier le kit dans la VM

Copie le dépôt vers `C:\macro-port` dans la VM avec `robocopy` depuis le partage, sans `.git` ni `target`. Puis lance `Get-ChildItem C:\macro-port -Recurse | Unblock-File` pour retirer la marque « venant d'Internet ».

## 3. Préparer la VM

1. **Exclusion antivirus** (droits administrateur requis) : crée `C:\macro-port\corpus\fixtures\generated`, puis lance `Add-MpPreference -ExclusionPath "C:\macro-port\corpus\fixtures"`. Vérifie avec `Get-MpPreference`. Si la commande sans `--current-user` n'a pas les droits administrateur, demande-moi d'ajouter l'exclusion à la main, en me donnant le chemin exact.
2. **Accès au modèle objet VBA** (avec `--current-user`) : repère la version d'Office sous `HKCU:\Software\Microsoft\Office\` (en principe `16.0`). Mets la valeur DWORD `AccessVBOM` à 1 dans `HKCU:\Software\Microsoft\Office\16.0\Excel\Security`. Note la valeur d'origine pour la restaurer ensuite.
3. Vérifie qu'aucun processus `EXCEL.EXE` ne tourne. S'il y en a un, demande-moi avant de le fermer : j'ai peut-être un classeur ouvert.

## 4. Lancer la fabrication

Avec `--current-user`, lance :

```
powershell.exe -ExecutionPolicy Bypass -File C:\macro-port\tools\fixtures\Build-Fixtures.ps1 -CorpusRoot C:\macro-port\corpus -OutDir C:\macro-port\corpus\fixtures\generated *> C:\macro-port\build-fixtures.log
```

Le traitement dure 15 à 30 minutes. Si `prlctl exec` risque d'expirer, lance le script en arrière-plan (par exemple une tâche planifiée immédiate dans ma session, ou `Start-Process` avec redirection vers le journal), puis surveille le journal toutes les 2 minutes.

Résultat attendu :

- 77 échantillons `.bas` et `.cls`, chacun en trois formats, soit 231 classeurs. Les 3 `.frm` sont volontairement ignorés.
- Un fichier `.expected.bas` ou `.expected.cls` par classeur.
- `enc-01-open-password.xlsm`, `enc-02-open-password.xls`, `xlm-01-macrosheet.xlsm`, `xlm-01-macrosheet.xls`, `none-01-no-macros.xlsm`.
- `corpus/fixtures/fixtures.yaml`.

Compte les fichiers et extrais du journal tous les avertissements « Skipped ». Si plus de 10 % des échantillons échouent, arrête-toi et montre-moi les erreurs avant de continuer.

## 5. Protection manuelle de trois projets VBA (étape humaine)

Cette protection ne peut pas se faire par automatisation fiable. Demande-moi de protéger ces trois classeurs dans la VM, en me rappelant la procédure : ouvrir le fichier, Alt+F11, Outils > Propriétés de VBAProject > Protection, cocher « Verrouiller le projet pour l'affichage », mot de passe `macro-port-test`, OK, enregistrer, fermer.

- `C:\macro-port\corpus\fixtures\generated\01-01-*.xlsm`
- `C:\macro-port\corpus\fixtures\generated\08-01-*.xlsm`
- `C:\macro-port\corpus\fixtures\generated\12-02-*.xlsm`

Attends ma confirmation. Ajoute ensuite `expected_protected: true` aux trois entrées de `fixtures.yaml`.

## 6. Remettre la VM en état

1. Remets `AccessVBOM` à sa valeur d'origine.
2. Vérifie qu'aucun `EXCEL.EXE` lancé par le script ne reste actif.
3. Copie `C:\macro-port\corpus\fixtures\generated\` et `C:\macro-port\corpus\fixtures\fixtures.yaml` vers le dépôt du Mac, dans `corpus/fixtures/`, par le partage. Copie aussi `build-fixtures.log` dans `corpus/fixtures/build-fixtures.log`.
4. Supprime `C:\macro-port` et `C:\macro-port-check` dans la VM, puis retire l'exclusion antivirus (`Remove-MpPreference -ExclusionPath "C:\macro-port\corpus\fixtures"`) et vérifie qu'elle n'apparaît plus.

## 7. Côté Mac : fichiers corrompus et contrôles

1. Lance `python3 -I tools/fixtures/make_malformed.py corpus/fixtures/generated corpus/fixtures/malformed`, puis vérifie que `corpus/fixtures/malformed/expected.yaml` existe.
2. Contrôles de cohérence :
   - chaque entrée de `fixtures.yaml` pointe vers un fichier présent ;
   - chaque `expected_source` existe ;
   - aucun fichier de `generated/` n'est absent du manifeste ;
   - le YAML se charge sans erreur.
3. Ajoute un `.gitattributes` qui marque `*.xlsm`, `*.xlsb`, `*.xls` et `*.xlsx` comme binaires, s'il n'existe pas déjà.

## 8. Livraison

Crée une branche `fixtures/initial`, commite, puis ouvre une demande de fusion en suivant le format de `CLAUDE.md`. Le résumé en français en tête doit couvrir :

- ce qui a été généré, avec les nombres ;
- les échantillons ignorés et pourquoi ;
- la version d'Excel et la page de code relevées dans `fixtures.yaml` ;
- les écarts éventuels par rapport à cette consigne.

Ne fusionne pas toi-même.

Termine par un compte rendu court : ce qui a marché, ce qui a échoué, et l'état final de la VM (exclusion retirée, AccessVBOM restauré, dossiers supprimés).
