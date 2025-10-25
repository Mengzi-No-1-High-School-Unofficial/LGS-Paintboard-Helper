import pandas as pd
import json

df = pd.read_csv("tokens.csv")

results: list[dict[str, str | int]] = []

for idx, row in df.iterrows():
    if row['UID'] is None or not isinstance(row['PaintKey'], str):
        continue
    
    result = {
        'uid': row['UID'],
        'token': row['PaintKey']
    }
    
    results.append(result)
    
    df['状态'][idx] = '已部署'
    
with open("exported_tokens.json", "w", encoding="utf-8") as f:
    json.dump(results, f, ensure_ascii=False, indent=4)
    
print(df)

input("Press Enter to save...")

df.to_csv("tokens.csv", index=False)