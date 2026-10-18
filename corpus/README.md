# vba-migration-corpus

Corpus de test de macros VBA Excel réelles, classées par grande famille de cas d'usage. Il sert à tester un analyseur de macros pour la migration vers OnlyOffice Spreadsheet, Grist et LibreOffice Calc.

Version 0.1, constituée le 17 octobre 2026.

## Contenu

- 80 échantillons de code (.bas, .cls, .frm) répartis en 14 familles, soit environ 9 900 lignes de code utiles.
- 11 entrées en référence seule : des sources intéressantes mais sans licence permettant la redistribution. Seule l'URL est conservée.
- `manifest.json` et `manifest.csv` : une ligne par échantillon. On y trouve la source, l'auteur, la licence, le nombre de lignes, les API VBA notables, la cible de migration pressentie, la difficulté, les points bloquants et une description.
- `tools/build_manifest.py` : agrège les manifestes des familles et vérifie le corpus (identifiants uniques, fichiers présents, en-têtes, doublons).

| Famille | Échantillons | Réf. seules |
| --- | --- | --- |
| 01-mise-en-forme | 6 | 0 |
| 02-consolidation | 6 | 2 |
| 03-nettoyage-transformation | 6 | 1 |
| 04-recherche-referentiel | 6 | 3 |
| 05-formulaires-saisie | 5 | 1 |
| 06-generation-rapports | 5 | 1 |
| 07-tcd-graphiques | 5 | 1 |
| 08-fonctions-personnalisees | 6 | 0 |
| 09-evenements | 6 | 0 |
| 10-fichiers-systeme | 6 | 0 |
| 11-donnees-externes | 5 | 1 |
| 12-interop-office | 6 | 1 |
| 13-api-windows-activex | 6 | 0 |
| 14-patterns-securite | 6 | 0 |

## Statuts

- `included` : fichier source complet, copié tel quel.
- `extract` : une procédure extraite telle quelle d'un module plus large, ou un bloc de code d'une page de documentation.
- `synthetic` : échantillon écrit pour le corpus (CC0) afin de combler un manque. Cela concerne toute la famille 14 ainsi que 04-09 et 12-07.
- `reference_only` : pas de fichier, URL seulement.

## Licences

Chaque fichier commence par un en-tête de commentaires VBA qui indique la source, l'auteur, la licence et la date de collecte. La répartition des licences est la suivante :

- MIT : 66 échantillons, majoritairement issus de MicrosoftDocs/VBA-Docs (code sous MIT) et de dépôts GitHub sous MIT, avec des liens figés sur un commit.
- CC0 : 8 échantillons synthétiques.
- GPL-2.0 : 2 échantillons.
- GPL-3.0 : 2 échantillons.
- MPL-2.0 : 2 échantillons.

Les échantillons sous GPL et MPL sont signalés dans le manifeste. Les métadonnées du corpus (manifestes, README) sont sous CC BY 4.0.

## Précautions d'usage

- **Le code est volontairement laissé tel quel.** Plusieurs fichiers contiennent des erreurs de compilation ou des bugs d'origine (01-06, 02-04, 05-04, 06-02, 08-04, 08-05, 09-03). Ce sont de bons cas de test pour la robustesse d'un analyseur.
- **Certains échantillons dépendent de modules absents du corpus.** Ces dépendances sont listées dans `blockers`.
- **Les fichiers .cls et .frm ne s'importent pas directement.** L'en-tête de commentaires placé en tête doit être retiré avant import dans l'éditeur VBA. Les fichiers .frx (binaires des formulaires) ne sont pas fournis.
- **La famille 14 ne contient aucun logiciel malveillant.** Ce sont des motifs bénins (exécution automatique, Shell, obfuscation par Chr, téléchargement vers example.com, secrets en clair, accès à VBProject, macros XLM) qui servent à tester la détection.
- **Les champs `expected_target` et `migration_difficulty` sont un premier jugement à expert.** Ils sont à revoir collectivement : ils serviront de vérité terrain pour évaluer la classification automatique.

## Limites connues de la version 0.1

- Aucun échantillon Stack Overflow : le site n'était pas accessible depuis l'environnement de collecte.
- Aucun classeur .xlsm binaire, uniquement des modules texte. Un lot ultérieur devra fournir des conteneurs réels, y compris protégés par mot de passe ou réduits au p-code.
- Peu de code « métier » d'administration publique. La priorité du lot suivant est d'obtenir des classeurs réels anonymisés auprès de partenaires.
